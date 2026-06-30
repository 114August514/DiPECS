//! Daemon pipeline integration tests.
//!
//! These tests drive the same building blocks that `dipecsd` uses —
//! `RustCollectorIngress → DefaultPrivacyAirGap → WindowAggregator →
//! DecisionRouter → PolicyEngine → ActionLifecycle` — with synthetic
//! Android-shaped JSONL lines and assert the full pipeline behaves correctly
//! for common emulator validation scenarios.

use aios_action::DefaultActionExecutor;
use aios_agent::DecisionRouter;
use aios_collector::AndroidJsonlIngress;
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::collector_ingress::RustCollectorIngress;
use aios_core::context_builder::WindowAggregator;
use aios_core::policy_engine::PolicyEngine;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_spec::governance::ActionState;
use aios_spec::traits::PrivacySanitizer;
use aios_spec::{CapabilityLevel, ContextSummary, SanitizedEventType, SemanticHint};

// ── JSONL fixture lines ──────────────────────────────────────────

const APP_TRANSITION_LINE: &str = r#"{"eventId":"evt-1","timestampMs":1000,"source":"UsageCollector","eventType":"app_transition","rawEvent":{"AppTransition":{"timestamp_ms":1000,"package_name":"com.android.chrome","activity_class":"MainActivity","transition":"Foreground"}},"rawPayload":{}}"#;

const NOTIFICATION_FILE_LINE: &str = r#"{"eventId":"evt-2","timestampMs":2000,"source":"NotificationCollectorService","eventType":"notification_posted","rawEvent":{"NotificationPosted":{"timestamp_ms":2000,"package_name":"com.ss.android.lark","category":"msg","channel_id":"lark_im_message","raw_title":"Zhang San","raw_text":"sent a file: report.pdf","is_ongoing":false,"group_key":"conv_42","has_picture":false}},"rawPayload":{}}"#;

const NOTIFICATION_VERIFY_LINE: &str = r#"{"eventId":"evt-3","timestampMs":3000,"source":"NotificationCollectorService","eventType":"notification_posted","rawEvent":{"NotificationPosted":{"timestamp_ms":3000,"package_name":"com.bank.app","category":"msg","channel_id":"verify","raw_title":"verification code","raw_text":"Your verification code is 654321","is_ongoing":false,"group_key":null,"has_picture":false}},"rawPayload":{}}"#;

const SYSTEM_LOW_BATTERY_LINE: &str = r#"{"eventId":"evt-4","timestampMs":4000,"source":"CollectorForegroundService","eventType":"system_state","rawEvent":{"SystemState":{"timestamp_ms":4000,"battery_pct":8,"is_charging":false,"network":"Wifi","ringer_mode":"Normal","location_type":"Unknown","headphone_connected":false,"bluetooth_connected":false}},"rawPayload":{}}"#;

const SCREEN_INTERACTIVE_LINE: &str = r#"{"eventId":"evt-5","timestampMs":5000,"source":"CollectorForegroundService","eventType":"screen_state","rawEvent":{"ScreenState":{"timestamp_ms":5000,"state":"Interactive"}},"rawPayload":{}}"#;

const NOTIFICATION_INTERACTION_LINE: &str = r#"{"eventId":"evt-6","timestampMs":6000,"source":"NotificationCollectorService","eventType":"notification_interaction","rawEvent":{"NotificationInteraction":{"timestamp_ms":6000,"package_name":"com.ss.android.lark","notification_key":"0|com.ss.android.lark|42|null|10086","action":"Tapped"}},"rawPayload":{}}"#;

const ACCESSIBILITY_NULL_RAW_LINE: &str = r#"{"eventId":"evt-7","timestampMs":7000,"source":"AccessibilityCollectorService","eventType":"accessibility_text","rawEvent":null,"rawPayload":{}}"#;

const APP_BACKGROUND_LINE: &str = r#"{"eventId":"evt-8","timestampMs":15000,"source":"UsageCollector","eventType":"app_transition","rawEvent":{"AppTransition":{"timestamp_ms":15000,"package_name":"com.android.chrome","activity_class":"MainActivity","transition":"Background"}},"rawPayload":{}}"#;

// ── Helpers ──────────────────────────────────────────────────────

fn run_pipeline(jsonl_lines: &[&str], window_secs: u64) -> PipelineResult {
    let ingress = RustCollectorIngress;
    let sanitizer = DefaultPrivacyAirGap;
    let router = DecisionRouter::default();
    let policy = PolicyEngine::default();
    let executor = DefaultActionExecutor::new();
    let lifecycle = ActionLifecycle::new(&policy, &executor);
    let jsonl_ingress = AndroidJsonlIngress::new();

    let mut aggregator: Option<WindowAggregator> = None;
    let mut last_timestamp_ms = 0i64;
    let mut window_ordinal = 0u32;
    let mut result = PipelineResult::default();

    for line in jsonl_lines {
        let envelope = match jsonl_ingress.parse_line(line) {
            Ok(Some(env)) => env,
            Ok(None) => {
                result.skipped_null_raw += 1;
                continue;
            },
            Err(_) => {
                result.parse_errors += 1;
                continue;
            },
        };
        let captured_at_ms = envelope.captured_at_ms;
        last_timestamp_ms = last_timestamp_ms.max(captured_at_ms);

        let ingested = ingress
            .accept(envelope)
            .expect("ingress.accept should succeed for valid envelope");
        result.events_ingested += 1;

        // Open or rotate window based on trace timestamps.
        let agg =
            aggregator.get_or_insert_with(|| WindowAggregator::new(window_secs, captured_at_ms));
        if agg.is_expired(captured_at_ms) {
            if let Some(ctx) = agg.close(captured_at_ms) {
                process_one_window(window_ordinal, &ctx, &router, &lifecycle, &mut result);
                window_ordinal += 1;
            }
        }

        let sanitized = sanitizer.sanitize_with_tier(ingested.raw_event, ingested.source_tier);
        result.sanitized_events.push(sanitized.clone());

        if let Some(agg) = aggregator.as_mut() {
            agg.push(sanitized);
        }
    }

    // Flush last window.
    if let Some(mut agg) = aggregator {
        if let Some(ctx) = agg.close(last_timestamp_ms) {
            process_one_window(window_ordinal, &ctx, &router, &lifecycle, &mut result);
        }
    }

    result
}

fn process_one_window(
    window_ordinal: u32,
    ctx: &aios_spec::StructuredContext,
    router: &DecisionRouter,
    lifecycle: &ActionLifecycle,
    result: &mut PipelineResult,
) {
    result.windows_closed += 1;
    result.window_summaries.push(ctx.summary.clone());

    let decision = router.evaluate(ctx);
    result.intents_total += decision.intent_batch.intents.len() as u64;

    let capability = CapabilityLevel::for_route(decision.route);
    let audit_records = lifecycle.run(
        window_ordinal,
        &decision.intent_batch,
        decision.route,
        decision.error.clone(),
        &capability,
        ctx,
    );

    for record in &audit_records {
        result.audit_records_total += 1;
        match record.terminal {
            ActionState::Succeeded => result.actions_succeeded += 1,
            ActionState::Failed => result.actions_failed += 1,
            ActionState::RejectedInvalidSchema
            | ActionState::DeniedByCapability
            | ActionState::DeniedByPolicy => result.actions_denied += 1,
            _ => {},
        }
    }

    if audit_records
        .iter()
        .any(|r| matches!(r.terminal, ActionState::Succeeded))
    {
        result.windows_with_actions += 1;
    }

    result.all_audit_records.extend(audit_records);
}

#[derive(Debug, Default)]
struct PipelineResult {
    skipped_null_raw: u64,
    parse_errors: u64,
    events_ingested: u64,
    windows_closed: u64,
    intents_total: u64,
    audit_records_total: u64,
    actions_succeeded: u64,
    actions_failed: u64,
    actions_denied: u64,
    windows_with_actions: u64,
    sanitized_events: Vec<aios_spec::SanitizedEvent>,
    window_summaries: Vec<ContextSummary>,
    all_audit_records: Vec<aios_spec::governance::AuditRecord>,
}

// ── Tests ────────────────────────────────────────────────────────

#[test]
fn null_raw_event_rows_are_skipped() {
    let result = run_pipeline(&[ACCESSIBILITY_NULL_RAW_LINE], 10);
    assert_eq!(result.skipped_null_raw, 1);
    assert_eq!(result.events_ingested, 0);
}

#[test]
fn single_app_transition_produces_context_with_foreground_app() {
    let result = run_pipeline(&[APP_TRANSITION_LINE], 10);
    assert_eq!(result.events_ingested, 1);
    assert_eq!(result.windows_closed, 1);
    assert_eq!(result.sanitized_events.len(), 1);

    let summary = &result.window_summaries[0];
    assert!(
        summary
            .foreground_apps
            .contains(&"com.android.chrome".to_string()),
        "foreground_apps should contain chrome, got {:?}",
        summary.foreground_apps
    );
}

#[test]
fn notification_with_file_mention_preserves_semantic_hint_after_sanitization() {
    let result = run_pipeline(&[NOTIFICATION_FILE_LINE], 10);
    assert_eq!(result.sanitized_events.len(), 1);

    match &result.sanitized_events[0].event_type {
        SanitizedEventType::Notification { semantic_hints, .. } => {
            assert!(
                semantic_hints.contains(&SemanticHint::FileMention),
                "expected FileMention hint, got {:?}",
                semantic_hints
            );
        },
        other => panic!("expected Notification, got {:?}", other),
    }

    // Raw PII (sender name "Zhang San") must not appear in sanitized context.
    let summary = &result.window_summaries[0];
    let summary_text = format!("{:?}", summary);
    assert!(
        !summary_text.contains("Zhang San"),
        "raw PII must not leak into context summary"
    );
}

#[test]
fn verification_code_notification_sets_correct_semantic_hint() {
    let result = run_pipeline(&[NOTIFICATION_VERIFY_LINE], 10);

    match &result.sanitized_events[0].event_type {
        SanitizedEventType::Notification { semantic_hints, .. } => {
            assert!(
                semantic_hints.contains(&SemanticHint::VerificationCode),
                "expected VerificationCode hint, got {:?}",
                semantic_hints
            );
        },
        other => panic!("expected Notification, got {:?}", other),
    }

    // Raw verification code must be scrubbed.
    let context_debug = format!("{:?}", result.window_summaries);
    assert!(
        !context_debug.contains("654321"),
        "PII leaked through sanitizer"
    );
}

#[test]
fn low_battery_triggers_release_memory_intent_and_action() {
    let result = run_pipeline(&[SYSTEM_LOW_BATTERY_LINE], 10);

    assert!(result.windows_closed >= 1);
    // The rule-based backend should generate at least one intent.
    assert!(
        result.intents_total >= 1,
        "expected intents from low battery context"
    );

    // At least one ReleaseMemory or KeepAlive action should be authorized.
    let succeeded_actions: Vec<_> = result
        .all_audit_records
        .iter()
        .filter(|r| matches!(r.terminal, ActionState::Succeeded))
        .collect();
    assert!(
        !succeeded_actions.is_empty(),
        "expected at least one authorized action for low battery"
    );
}

#[test]
fn multi_event_window_aggregates_correctly() {
    // All events within 10s window.
    let lines = &[
        APP_TRANSITION_LINE,
        NOTIFICATION_FILE_LINE,
        SCREEN_INTERACTIVE_LINE,
    ];
    let result = run_pipeline(lines, 10);

    assert_eq!(result.events_ingested, 3);
    assert_eq!(result.windows_closed, 1);
    assert_eq!(result.sanitized_events.len(), 3);

    let summary = &result.window_summaries[0];
    assert!(summary
        .foreground_apps
        .contains(&"com.android.chrome".to_string()));
    assert!(summary
        .notified_apps
        .contains(&"com.ss.android.lark".to_string()));
}

#[test]
fn window_boundary_splits_events_correctly() {
    // Event at t=1000, then event at t=15000 → should produce 2 windows with
    // a 10-second window duration.
    let lines = &[APP_TRANSITION_LINE, APP_BACKGROUND_LINE];
    let result = run_pipeline(lines, 10);

    assert_eq!(result.events_ingested, 2);
    assert_eq!(result.windows_closed, 2);
}

#[test]
fn notification_interaction_sanitizes_to_notification_type() {
    // NotificationInteraction raw events map to SanitizedEventType::Notification
    // after sanitization, not a separate variant.
    let result = run_pipeline(&[NOTIFICATION_INTERACTION_LINE], 10);

    assert_eq!(result.events_ingested, 1);
    assert_eq!(result.sanitized_events.len(), 1);

    match &result.sanitized_events[0].event_type {
        SanitizedEventType::Notification { source_package, .. } => {
            assert_eq!(source_package, "com.ss.android.lark");
        },
        other => panic!("expected Notification, got {:?}", other),
    }
}

#[test]
fn pipeline_is_deterministic() {
    let lines = &[
        APP_TRANSITION_LINE,
        NOTIFICATION_FILE_LINE,
        SYSTEM_LOW_BATTERY_LINE,
    ];
    let a = run_pipeline(lines, 10);
    let b = run_pipeline(lines, 10);

    assert_eq!(a.events_ingested, b.events_ingested);
    assert_eq!(a.windows_closed, b.windows_closed);
    assert_eq!(a.intents_total, b.intents_total);
    assert_eq!(a.actions_succeeded, b.actions_succeeded);
    assert_eq!(a.actions_denied, b.actions_denied);
    assert_eq!(a.audit_records_total, b.audit_records_total);
}

#[test]
fn pipeline_handles_empty_input_gracefully() {
    let result = run_pipeline(&[], 10);
    assert_eq!(result.events_ingested, 0);
    assert_eq!(result.windows_closed, 0);
    assert_eq!(result.audit_records_total, 0);
}

#[test]
fn pipeline_handles_parse_error_gracefully() {
    let result = run_pipeline(&["{not valid json"], 10);
    assert_eq!(result.parse_errors, 1);
    assert_eq!(result.events_ingested, 0);
}

#[test]
fn multiple_windows_each_produce_audit_records() {
    // Spread events across 30s → 3 windows (10s each) at t=1000, t=12000, t=25000
    let early = r#"{"eventId":"e1","timestampMs":1000,"source":"UsageCollector","eventType":"app_transition","rawEvent":{"AppTransition":{"timestamp_ms":1000,"package_name":"com.a","activity_class":"M","transition":"Foreground"}},"rawPayload":{}}"#;
    let mid = r#"{"eventId":"e2","timestampMs":12000,"source":"UsageCollector","eventType":"app_transition","rawEvent":{"AppTransition":{"timestamp_ms":12000,"package_name":"com.b","activity_class":"M","transition":"Foreground"}},"rawPayload":{}}"#;
    let late = r#"{"eventId":"e3","timestampMs":25000,"source":"UsageCollector","eventType":"app_transition","rawEvent":{"AppTransition":{"timestamp_ms":25000,"package_name":"com.c","activity_class":"M","transition":"Foreground"}},"rawPayload":{}}"#;

    let result = run_pipeline(&[early, mid, late], 10);

    assert_eq!(result.events_ingested, 3);
    assert_eq!(result.windows_closed, 3);
    assert!(
        result.audit_records_total >= 3,
        "each window should produce at least one audit record"
    );
}
