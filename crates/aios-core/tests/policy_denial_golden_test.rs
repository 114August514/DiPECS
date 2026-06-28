//! Golden test for the *complete* denial surface of `PolicyEngine`.
//!
//! The end-to-end replay test in aios-cli can only reach the subset of
//! denial reasons that the real RuleBased / FallbackNoOp backends actually
//! produce. This test bypasses the routing layer and synthesises one
//! adversarial intent per `DenialReason` variant — proving each rule fires,
//! pinning the resulting count map.
//!
//! Update the expected map below in lock-step with any change to the
//! denial-reason enum or to which rule each variant maps to.

use std::collections::BTreeMap;

use aios_core::policy_engine::{PolicyConfig, PolicyEngine};
use aios_spec::governance::{PolicyActionDecision, PolicyVerdict};
use aios_spec::*;

fn ctx_with_packages(pkgs: &[&str]) -> StructuredContext {
    StructuredContext {
        window_id: "w-denial".into(),
        window_start_ms: 0,
        window_end_ms: 10_000,
        duration_secs: 10,
        events: vec![],
        summary: ContextSummary {
            foreground_apps: pkgs.iter().map(|s| (*s).into()).collect(),
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    }
}

fn intent(id: &str, risk: RiskLevel, confidence: f32, actions: Vec<SuggestedAction>) -> Intent {
    Intent {
        intent_id: id.into(),
        intent_type: IntentType::Idle,
        confidence,
        risk_level: risk,
        suggested_actions: actions,
        rationale_tags: vec![],
    }
}

fn action(
    action_type: ActionType,
    target: Option<&str>,
    urgency: ActionUrgency,
) -> SuggestedAction {
    SuggestedAction {
        action_type,
        target: target.map(String::from),
        urgency,
    }
}

fn batch(intents: Vec<Intent>) -> IntentBatch {
    IntentBatch {
        window_id: "w-denial".into(),
        intents,
        generated_at_ms: 10_000,
        model: "synthetic-adversarial".into(),
    }
}

/// Aggregate the per-decision denial signals (intent-level rejection_reason
/// and per-action action_denials) into a single histogram, mirroring how
/// `ReplaySummary.denial_counts` is built in the CLI.
fn aggregate(decisions: &[PolicyActionDecision]) -> BTreeMap<DenialReason, u64> {
    let mut counts: BTreeMap<DenialReason, u64> = BTreeMap::new();
    for d in decisions {
        if let PolicyVerdict::Denied(r) = d.verdict {
            *counts.entry(r).or_insert(0) += 1;
        }
    }
    counts
}

#[test]
fn every_denial_reason_fires_exactly_once_against_synthetic_catalog() {
    // Engine: Low-risk-only with NoOp on the blocked list and a tight
    // per-intent action cap so each rule has a reachable triggering combo.
    let config = PolicyConfig {
        max_auto_risk: RiskLevel::Low,
        blocked_actions: vec!["NoOp".into()],
        max_actions_per_batch: 1,
        min_confidence: 0.3,
    };
    let engine = PolicyEngine::new(config);

    // CloudLlm capability: max_risk Medium, allows the full action set —
    // exposes config-level gates and per-action gates that aren't a
    // capability mismatch.
    let cloud = CapabilityLevel::for_route(DecisionRoute::CloudLlm);
    // FallbackNoOp capability: max_risk Low, only NoOp in allow_list —
    // exposes capability-level risk and action gates.
    let restricted = CapabilityLevel::for_route(DecisionRoute::FallbackNoOp);

    let ctx = ctx_with_packages(&["com.known"]);

    // --- One intent per denial reason. ---

    // 1. RiskExceedsConfig — Medium passes Cloud's Medium cap, fails engine
    //    config's Low cap.
    let r_exceeds_config = intent(
        "i-risk-cfg",
        RiskLevel::Medium,
        0.9,
        vec![action(
            ActionType::ReleaseMemory,
            None,
            ActionUrgency::Immediate,
        )],
    );

    // 2. RiskExceedsCapability — Medium against FallbackNoOp's Low max:
    //    capability check fires *before* the engine-config check.
    let r_exceeds_cap = intent(
        "i-risk-cap",
        RiskLevel::Medium,
        0.9,
        vec![action(ActionType::NoOp, None, ActionUrgency::Immediate)],
    );

    // 3. ConfidenceTooLow — conf 0.1 < floor 0.3.
    let conf_too_low = intent(
        "i-conf",
        RiskLevel::Low,
        0.1,
        vec![action(ActionType::NoOp, None, ActionUrgency::Immediate)],
    );

    // 4. ActionTypeBlocked — engine-config blocked_actions contains "NoOp".
    let blocked = intent(
        "i-blocked",
        RiskLevel::Low,
        0.9,
        vec![action(ActionType::NoOp, None, ActionUrgency::Immediate)],
    );

    // 5. ActionUrgencyDeferred — Deferred KeepAlive on a known target.
    let deferred = intent(
        "i-deferred",
        RiskLevel::Low,
        0.9,
        vec![action(
            ActionType::KeepAlive,
            Some("com.known"),
            ActionUrgency::Deferred,
        )],
    );

    // 6. ActionCapabilityDenied — PreWarmProcess against FallbackNoOp
    //    (only NoOp in allow_list).
    let cap_denied = intent(
        "i-capdenied",
        RiskLevel::Low,
        0.9,
        vec![action(
            ActionType::PreWarmProcess,
            Some("com.known"),
            ActionUrgency::Immediate,
        )],
    );

    // 7. TargetNotInContext — KeepAlive against an unobserved package under
    //    CloudLlm (KeepAlive *is* in allow_list, so cap doesn't preempt).
    let target_unknown = intent(
        "i-target",
        RiskLevel::Low,
        0.9,
        vec![action(
            ActionType::KeepAlive,
            Some("com.never-seen"),
            ActionUrgency::Immediate,
        )],
    );

    // 8. BatchActionCapExceeded — two valid actions, engine cap = 1: the
    //    second one is denied.
    let too_many = intent(
        "i-toomany",
        RiskLevel::Low,
        0.9,
        vec![
            action(
                ActionType::KeepAlive,
                Some("com.known"),
                ActionUrgency::Immediate,
            ),
            action(
                ActionType::KeepAlive,
                Some("com.known"),
                ActionUrgency::Immediate,
            ),
        ],
    );

    // CloudLlm covers config-level + action-level rules whose firing
    // requires the broader allow list and the higher cap headroom.
    let mut all = engine.evaluate_batch_with_context(
        &batch(vec![
            r_exceeds_config,
            conf_too_low,
            blocked,
            deferred,
            target_unknown,
            too_many,
        ]),
        &cloud,
        &ctx,
    );
    // Restricted covers the capability-level rules — risk *and* action.
    all.extend(engine.evaluate_batch_with_context(
        &batch(vec![r_exceeds_cap, cap_denied]),
        &restricted,
        &ctx,
    ));

    let counts = aggregate(&all);

    let mut expected: BTreeMap<DenialReason, u64> = BTreeMap::new();
    expected.insert(DenialReason::RiskExceedsConfig, 1);
    expected.insert(DenialReason::RiskExceedsCapability, 1);
    expected.insert(DenialReason::ConfidenceTooLow, 1);
    expected.insert(DenialReason::ActionTypeBlocked, 1);
    expected.insert(DenialReason::ActionUrgencyDeferred, 1);
    expected.insert(DenialReason::ActionCapabilityDenied, 1);
    expected.insert(DenialReason::TargetNotInContext, 1);
    expected.insert(DenialReason::BatchActionCapExceeded, 1);

    assert_eq!(
        counts, expected,
        "denial-reason histogram drifted. each variant of DenialReason must \
         fire exactly once under this synthetic catalog — if you added a new \
         variant, add a fixture intent for it above."
    );
}
