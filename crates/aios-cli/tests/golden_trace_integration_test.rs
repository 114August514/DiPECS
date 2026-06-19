//! GoldenTrace integration test — closes the determinism loop.
//!
//! Every prior slice pinned a *property* of the pipeline (audit hash,
//! denial counts, privacy-leak absence). This test pins the
//! *observable input → output map* of the whole pipeline through the
//! `GoldenTrace` shape from `aios_spec::trace`, driving the actual
//! sanitizer / router / policy / executor and comparing against an
//! expected `(sanitized, intents, executed)` triple committed to
//! `data/traces/golden_sample.json`.
//!
//! Equality is *semantic*: volatile fields (uuids, wall-clock timestamps)
//! are deliberately ignored so the test stays meaningful under uuid churn.
//! For byte-exact pinning use `ReplaySummary.audit_hash` instead.
//!
//! ## Regenerating the fixture
//!
//! ```bash
//! REGEN_GOLDEN=1 cargo test --test golden_trace_integration_test regen_golden_sample
//! ```
//!
//! This rewrites `data/traces/golden_sample.json` from the current
//! pipeline output. Use it when policy, sanitization, or backend rules
//! intentionally change.

use std::fs;
use std::path::PathBuf;

use aios_action::DefaultActionExecutor;
use aios_agent::DecisionRouter;
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::context_builder::WindowAggregator;
use aios_core::policy_engine::PolicyEngine;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_core::trace_engine::DefaultTraceEngine;
use aios_spec::traits::{PrivacySanitizer, TraceValidator};
use aios_spec::{
    AppTransition, AppTransitionRawEvent, CapabilityLevel, ExecutedAction, FsAccessEvent,
    FsAccessType, GoldenTrace, IntentBatch, LocationType, NetworkType, RawEvent, RingerMode,
    SanitizedEvent, SystemStateEvent,
};

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/traces/golden_sample.json")
}

/// The exact same RawEvent sequence the GoldenTrace pins. Authored in
/// code so the regen path and the validation path share a single source
/// of truth for the inputs — the golden file is then just the captured
/// output side.
fn fixture_raw_events() -> Vec<RawEvent> {
    vec![
        RawEvent::AppTransition(AppTransitionRawEvent {
            timestamp_ms: 1_000,
            package_name: "com.android.chrome".into(),
            activity_class: Some("MainActivity".into()),
            transition: AppTransition::Foreground,
        }),
        RawEvent::FileSystemAccess(FsAccessEvent {
            timestamp_ms: 2_000,
            pid: 42,
            uid: 10_042,
            file_path: "/storage/emulated/0/DCIM/photo.jpg".into(),
            access_type: FsAccessType::OpenRead,
            bytes_transferred: Some(4_096),
        }),
        RawEvent::SystemState(SystemStateEvent {
            timestamp_ms: 3_000,
            battery_pct: Some(10),
            is_charging: false,
            network: NetworkType::Wifi,
            ringer_mode: RingerMode::Normal,
            location_type: LocationType::Unknown,
            headphone_connected: false,
            bluetooth_connected: false,
        }),
    ]
}

/// Drive the full pipeline on `fixture_raw_events()` and return the
/// observable outputs (sanitized events, intent batch, executed actions).
/// This is the *single* place that knows how to wire the components.
fn drive_pipeline() -> (Vec<SanitizedEvent>, IntentBatch, Vec<ExecutedAction>) {
    let raw_events = fixture_raw_events();
    let sanitizer = DefaultPrivacyAirGap;
    let sanitized: Vec<SanitizedEvent> = raw_events
        .iter()
        .cloned()
        .map(|r| sanitizer.sanitize(r))
        .collect();

    let mut agg = WindowAggregator::new(10, 1_000);
    for s in &sanitized {
        agg.push(s.clone());
    }
    let ctx = agg
        .close(3_000)
        .expect("non-empty window must produce a StructuredContext");

    let router = DecisionRouter::default();
    let decision = router.evaluate(&ctx);
    let capability = CapabilityLevel::for_route(decision.route);

    let policy = PolicyEngine::default();
    let executor = DefaultActionExecutor::new();
    let lifecycle = ActionLifecycle::new(&policy, &executor);
    let audit_records = lifecycle.run(0, &decision.intent_batch, &capability, &ctx);

    let mut executed: Vec<ExecutedAction> = Vec::new();
    for record in &audit_records {
        if !matches!(
            record.terminal,
            aios_spec::governance::ActionState::Succeeded
        ) {
            continue;
        }
        let summary = record
            .outcome
            .as_ref()
            .map(|o| o.summary.clone())
            .unwrap_or_else(|| "ok".into());
        executed.push(ExecutedAction {
            action_type: format!("{:?}", record.action_type),
            target: record.target.clone(),
            executed_at_ms: ctx.window_end_ms,
            success: true,
            error_reason: None,
        });
        // Keep `summary` reachable for future extensions without a warning.
        let _ = summary;
    }

    (sanitized, decision.intent_batch, executed)
}

fn load_golden() -> GoldenTrace {
    let bytes = fs::read(golden_path()).unwrap_or_else(|e| {
        panic!(
            "golden file missing at {}: {e}. Run with REGEN_GOLDEN=1 to create it.",
            golden_path().display()
        )
    });
    serde_json::from_slice(&bytes).expect("golden file is valid GoldenTrace JSON")
}

#[test]
fn replay_matches_golden_sample() {
    let golden = load_golden();
    let (sanitized, intents, executed) = drive_pipeline();

    let engine = DefaultTraceEngine::new(DefaultPrivacyAirGap);
    let result = engine.validate(&golden, &intents, &executed);

    assert!(
        result.sanitization_match,
        "sanitization divergences at indices {:?} — pipeline drifted from \
         golden expected_sanitized; if intentional, REGEN_GOLDEN=1",
        result.sanitization_divergences
    );
    assert!(
        result.policy_match,
        "policy divergences: {:#?} — intent batch drifted from golden \
         expected_intents; if intentional, REGEN_GOLDEN=1",
        result.policy_divergences
    );
    assert!(
        result.execution_match,
        "execution divergences at indices {:?} — executor drifted from \
         golden expected_actions; if intentional, REGEN_GOLDEN=1",
        result.execution_divergences
    );
    assert!(result.all_match());

    // Sanity: the golden's own counts are what we expect (denial.jsonl
    // shape — 2 ActionCapabilityDenied → 2 approved actions executed).
    assert_eq!(
        sanitized.len(),
        3,
        "fixture has 3 raw events, sanitizer is 1:1"
    );
    assert_eq!(
        executed.len(),
        2,
        "exactly 2 actions survive policy: KeepAlive(com.android.chrome) + ReleaseMemory(None)"
    );
}

#[test]
fn mutated_expected_sanitized_is_flagged() {
    let mut golden = load_golden();
    let (sanitized, intents, executed) = drive_pipeline();

    // Flip a structural field on the *expected* side and confirm the
    // engine catches it. We pick `app_package` because `sanitized_eq`
    // compares it explicitly.
    assert!(
        !golden.expected_sanitized.is_empty(),
        "golden has at least one expected sanitized event"
    );
    golden.expected_sanitized[0].app_package = Some("com.intentional-drift".into());

    let engine = DefaultTraceEngine::new(DefaultPrivacyAirGap);
    let result = engine.validate(&golden, &intents, &executed);

    assert!(!result.sanitization_match);
    assert_eq!(
        result.sanitization_divergences,
        vec![0],
        "the mutation was on index 0 — engine should pinpoint it"
    );
    // Policy and execution should still match (we didn't touch them).
    assert!(result.policy_match);
    assert!(result.execution_match);
    assert!(
        !result.all_match(),
        "all_match() must be false when any layer diverges"
    );

    // Unused binding kept around for clarity — `sanitized` is what the
    // pipeline actually produced; the test mutates the *expected* side.
    let _ = sanitized;
}

#[test]
fn mutated_rationale_tags_is_flagged() {
    // rationale_tags is part of observable intent output — drift there is
    // real (the RuleBased backend encodes its reasoning into tags like
    // "low_battery"), so the engine must surface it as a policy divergence.
    let mut golden = load_golden();
    let (_, intents, executed) = drive_pipeline();

    assert!(
        !golden.expected_intents.intents.is_empty(),
        "golden has at least one expected intent"
    );
    golden.expected_intents.intents[0]
        .rationale_tags
        .push("intentional-drift".into());

    let engine = DefaultTraceEngine::new(DefaultPrivacyAirGap);
    let result = engine.validate(&golden, &intents, &executed);

    assert!(!result.policy_match);
    assert!(
        result
            .policy_divergences
            .iter()
            .any(|d| d.contains("rationale_tags")),
        "expected a rationale_tags divergence, got {:#?}",
        result.policy_divergences
    );
}

#[test]
fn validate_sanitization_marks_unchecked_layers_false() {
    // The sanitization-only entry point must not silently report
    // policy_match/execution_match as true — that would let callers
    // misread "not checked" as "passed". The contract is: match flags
    // answer "did this layer pass"; divergence lists answer "if it
    // failed, where". Unchecked layers therefore have match=false AND
    // empty divergence lists (not a sentinel string — that would
    // conflate "not checked" with "checked and these are the problems").
    let golden = load_golden();

    let engine = DefaultTraceEngine::new(DefaultPrivacyAirGap);
    let result = engine.validate_sanitization(&golden);

    assert!(result.sanitization_match);
    assert!(!result.policy_match);
    assert!(!result.execution_match);
    assert!(
        result.policy_divergences.is_empty(),
        "unchecked layers must have empty divergence lists, not sentinels"
    );
    assert!(result.execution_divergences.is_empty());
    assert!(
        !result.all_match(),
        "all_match() must be false when policy/execution were not actually checked"
    );
}

#[test]
fn mutated_expected_intent_count_is_flagged() {
    let mut golden = load_golden();
    let (_, intents, executed) = drive_pipeline();

    // Drop one expected intent — the engine should flag a count mismatch.
    let original_count = golden.expected_intents.intents.len();
    assert!(
        original_count > 0,
        "golden should have at least one expected intent"
    );
    golden.expected_intents.intents.pop();

    let engine = DefaultTraceEngine::new(DefaultPrivacyAirGap);
    let result = engine.validate(&golden, &intents, &executed);

    assert!(!result.policy_match);
    assert!(
        result
            .policy_divergences
            .iter()
            .any(|d| d.contains("intent count mismatch")),
        "expected an intent-count mismatch divergence, got {:#?}",
        result.policy_divergences
    );
}

/// Regenerate `data/traces/golden_sample.json` from the current pipeline.
/// No-op unless `REGEN_GOLDEN=1` is set in the environment, so plain
/// `cargo test` never rewrites the committed golden.
#[test]
fn regen_golden_sample() {
    if std::env::var("REGEN_GOLDEN").ok().as_deref() != Some("1") {
        eprintln!("regen_golden_sample: set REGEN_GOLDEN=1 to rewrite the fixture");
        return;
    }
    let (sanitized, intents, executed) = drive_pipeline();
    let golden = GoldenTrace {
        trace_id: "golden-sample-v1".into(),
        window_start_ms: 1_000,
        window_end_ms: 3_000,
        raw_events: fixture_raw_events(),
        expected_sanitized: sanitized,
        expected_intents: intents,
        expected_actions: executed,
    };
    let json = serde_json::to_string_pretty(&golden).expect("GoldenTrace serializes");
    let path = golden_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create traces dir");
    }
    fs::write(&path, json).expect("write golden file");
    eprintln!("wrote {}", path.display());
}
