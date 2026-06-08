//! 验证 PolicyEngine 的意图校验逻辑

use aios_core::policy_engine::{PolicyConfig, PolicyEngine};
use aios_spec::*;

fn make_intent(
    id: &str,
    intent_type: IntentType,
    confidence: f32,
    risk: RiskLevel,
    actions: Vec<SuggestedAction>,
) -> Intent {
    Intent {
        intent_id: id.into(),
        intent_type,
        confidence,
        risk_level: risk,
        suggested_actions: actions,
        rationale_tags: vec![],
    }
}

fn make_action(
    action_type: ActionType,
    target: Option<&str>,
    urgency: ActionUrgency,
) -> SuggestedAction {
    SuggestedAction {
        action_type,
        target: target.map(|s| s.to_string()),
        urgency,
    }
}

fn make_batch(intents: Vec<Intent>) -> IntentBatch {
    IntentBatch {
        window_id: "w1".into(),
        intents,
        generated_at_ms: 5000,
        model: "test".into(),
    }
}

// ===== 风险等级检查 =====

#[test]
fn test_low_risk_approved_by_default() {
    let engine = PolicyEngine::default();
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.8,
        RiskLevel::Low,
        vec![make_action(ActionType::NoOp, None, ActionUrgency::IdleTime)],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch(&batch);

    assert_eq!(decisions.len(), 1);
    assert!(decisions[0].approved);
    assert!(decisions[0].rejection_reason.is_none());
}

#[test]
fn test_medium_risk_rejected_by_default() {
    let engine = PolicyEngine::default();
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.8,
        RiskLevel::Medium,
        vec![make_action(ActionType::NoOp, None, ActionUrgency::IdleTime)],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch(&batch);

    assert_eq!(decisions.len(), 1);
    assert!(!decisions[0].approved);
    assert!(decisions[0].rejection_reason.is_some());
}

#[test]
fn test_high_risk_rejected_by_default() {
    let engine = PolicyEngine::default();
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.8,
        RiskLevel::High,
        vec![make_action(ActionType::NoOp, None, ActionUrgency::IdleTime)],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch(&batch);

    assert_eq!(decisions.len(), 1);
    assert!(!decisions[0].approved);
}

#[test]
fn test_medium_risk_approved_with_relaxed_config() {
    let config = PolicyConfig {
        max_auto_risk: RiskLevel::Medium,
        ..PolicyConfig::default()
    };
    let engine = PolicyEngine::new(config);
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.8,
        RiskLevel::Medium,
        vec![make_action(ActionType::NoOp, None, ActionUrgency::IdleTime)],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch(&batch);

    assert!(decisions[0].approved);
}

// ===== 置信度检查 =====

#[test]
fn test_low_confidence_rejected() {
    let engine = PolicyEngine::default();
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.25,
        RiskLevel::Low,
        vec![make_action(ActionType::NoOp, None, ActionUrgency::IdleTime)],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch(&batch);

    assert!(!decisions[0].approved);
    assert_eq!(
        decisions[0].rejection_reason,
        Some(DenialReason::ConfidenceTooLow)
    );
}

#[test]
fn test_confidence_at_boundary_approved() {
    let engine = PolicyEngine::default();
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.3,
        RiskLevel::Low,
        vec![make_action(ActionType::NoOp, None, ActionUrgency::IdleTime)],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch(&batch);

    assert!(decisions[0].approved, "confidence 0.3 should be approved");
}

// ===== 动作过滤 =====

#[test]
fn test_deferred_urgency_filtered() {
    let engine = PolicyEngine::default();
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.8,
        RiskLevel::Low,
        vec![
            make_action(
                ActionType::PreWarmProcess,
                Some("com.a"),
                ActionUrgency::Deferred,
            ),
            make_action(ActionType::NoOp, None, ActionUrgency::Immediate),
        ],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch(&batch);

    assert!(decisions[0].approved);
    // Deferred action 被过滤, 只剩 NoOp
    assert_eq!(decisions[0].approved_actions.len(), 1);
    assert!(matches!(
        decisions[0].approved_actions[0].action.action_type,
        ActionType::NoOp
    ));
}

#[test]
fn test_blocked_action_filtered() {
    let config = PolicyConfig {
        blocked_actions: vec!["ReleaseMemory".into()],
        ..PolicyConfig::default()
    };
    let engine = PolicyEngine::new(config);
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.8,
        RiskLevel::Low,
        vec![
            make_action(ActionType::ReleaseMemory, None, ActionUrgency::Immediate),
            make_action(ActionType::NoOp, None, ActionUrgency::Immediate),
        ],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch(&batch);

    assert!(decisions[0].approved);
    assert_eq!(decisions[0].approved_actions.len(), 1);
    assert!(matches!(
        decisions[0].approved_actions[0].action.action_type,
        ActionType::NoOp
    ));
}

#[test]
fn test_max_actions_per_batch_enforced() {
    let config = PolicyConfig {
        max_actions_per_batch: 2,
        ..PolicyConfig::default()
    };
    let engine = PolicyEngine::new(config);
    let intent = make_intent(
        "i1",
        IntentType::OpenApp("com.a".into()),
        0.8,
        RiskLevel::Low,
        vec![
            make_action(
                ActionType::PreWarmProcess,
                Some("com.a"),
                ActionUrgency::Immediate,
            ),
            make_action(
                ActionType::KeepAlive,
                Some("com.a"),
                ActionUrgency::Immediate,
            ),
            make_action(
                ActionType::PrefetchFile,
                Some("/tmp"),
                ActionUrgency::Immediate,
            ),
        ],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch(&batch);

    assert!(decisions[0].approved);
    assert_eq!(
        decisions[0].approved_actions.len(),
        2,
        "should be capped at 2"
    );
}

// ===== 批量校验 =====

#[test]
fn test_evaluate_batch_mixed_results() {
    let engine = PolicyEngine::default();
    let intents = vec![
        make_intent(
            "i-ok",
            IntentType::Idle,
            0.9,
            RiskLevel::Low,
            vec![make_action(
                ActionType::NoOp,
                None,
                ActionUrgency::Immediate,
            )],
        ),
        make_intent(
            "i-high-risk",
            IntentType::Idle,
            0.9,
            RiskLevel::High,
            vec![make_action(
                ActionType::NoOp,
                None,
                ActionUrgency::Immediate,
            )],
        ),
        make_intent(
            "i-low-conf",
            IntentType::Idle,
            0.1,
            RiskLevel::Low,
            vec![make_action(
                ActionType::NoOp,
                None,
                ActionUrgency::Immediate,
            )],
        ),
    ];
    let batch = make_batch(intents);
    let decisions = engine.evaluate_batch(&batch);

    assert_eq!(decisions.len(), 3);
    assert!(
        decisions[0].approved,
        "low risk + high confidence → approved"
    );
    assert!(!decisions[1].approved, "high risk → rejected");
    assert!(!decisions[2].approved, "low confidence → rejected");
}

#[test]
fn test_approved_intent_preserves_actions() {
    let engine = PolicyEngine::default();
    let intent = make_intent(
        "i1",
        IntentType::SwitchToApp("com.a".into()),
        0.85,
        RiskLevel::Low,
        vec![
            make_action(
                ActionType::PreWarmProcess,
                Some("com.a"),
                ActionUrgency::Immediate,
            ),
            make_action(
                ActionType::KeepAlive,
                Some("com.a"),
                ActionUrgency::Immediate,
            ),
        ],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch(&batch);

    assert!(decisions[0].approved);
    assert_eq!(decisions[0].approved_actions.len(), 2);
    assert_eq!(decisions[0].approved_actions[0].intent_id, "i1");
    assert_eq!(decisions[0].approved_actions[0].authorized_at_ms, 5000);
    assert!(matches!(
        decisions[0].approved_actions[0].action.action_type,
        ActionType::PreWarmProcess
    ));
}

// ===== CapabilityLevel 检查 =====

#[test]
fn test_rule_based_backend_rejects_medium_risk() {
    let engine = PolicyEngine::default();
    let capability = CapabilityLevel::for_route(DecisionRoute::RuleBased);
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.8,
        RiskLevel::Medium,
        vec![make_action(
            ActionType::ReleaseMemory,
            None,
            ActionUrgency::Immediate,
        )],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch_with_capability(&batch, &capability);

    assert_eq!(decisions.len(), 1);
    assert!(
        !decisions[0].approved,
        "RuleBased backend should reject Medium risk"
    );
    assert_eq!(
        decisions[0].rejection_reason,
        Some(DenialReason::RiskExceedsCapability)
    );
}

#[test]
fn test_fallback_noop_blocks_prewarm() {
    let engine = PolicyEngine::default();
    let capability = CapabilityLevel::for_route(DecisionRoute::FallbackNoOp);
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.8,
        RiskLevel::Low,
        vec![
            make_action(
                ActionType::PreWarmProcess,
                Some("com.a"),
                ActionUrgency::Immediate,
            ),
            make_action(ActionType::NoOp, None, ActionUrgency::Immediate),
        ],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch_with_capability(&batch, &capability);

    assert!(decisions[0].approved);
    assert_eq!(decisions[0].approved_actions.len(), 1);
    assert!(matches!(
        decisions[0].approved_actions[0].action.action_type,
        ActionType::NoOp
    ));
    assert_eq!(decisions[0].action_denials.len(), 1);
    assert_eq!(
        decisions[0].action_denials[0],
        DenialReason::ActionCapabilityDenied
    );
}

#[test]
fn test_cloud_llm_allows_medium_risk() {
    let engine = PolicyEngine::default();
    let capability = CapabilityLevel::for_route(DecisionRoute::CloudLlm);
    let intent = make_intent(
        "i1",
        IntentType::OpenApp("com.a".into()),
        0.8,
        RiskLevel::Medium,
        vec![make_action(
            ActionType::PreWarmProcess,
            Some("com.a"),
            ActionUrgency::Immediate,
        )],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch_with_capability(&batch, &capability);

    // CloudLlm allows Medium, but config default only allows Low
    // So the config-level check should reject it
    assert!(!decisions[0].approved);
    assert_eq!(
        decisions[0].rejection_reason,
        Some(DenialReason::RiskExceedsConfig)
    );
}

#[test]
fn test_cloud_llm_medium_risk_with_relaxed_config() {
    let config = PolicyConfig {
        max_auto_risk: RiskLevel::Medium,
        ..PolicyConfig::default()
    };
    let engine = PolicyEngine::new(config);
    let capability = CapabilityLevel::for_route(DecisionRoute::CloudLlm);
    let intent = make_intent(
        "i1",
        IntentType::OpenApp("com.a".into()),
        0.8,
        RiskLevel::Medium,
        vec![make_action(
            ActionType::PreWarmProcess,
            Some("com.a"),
            ActionUrgency::Immediate,
        )],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch_with_capability(&batch, &capability);

    assert!(
        decisions[0].approved,
        "CloudLlm + relaxed config should allow Medium risk"
    );
    assert_eq!(decisions[0].approved_actions.len(), 1);
}

// ===== Target-not-in-context (only enforced via evaluate_batch_with_context) =====

fn ctx_with_packages(pkgs: &[&str]) -> StructuredContext {
    StructuredContext {
        window_id: "w-ctx".into(),
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

#[test]
fn test_target_in_context_approved() {
    let engine = PolicyEngine::default();
    let capability = CapabilityLevel::for_route(DecisionRoute::CloudLlm);
    let ctx = ctx_with_packages(&["com.known"]);
    let intent = make_intent(
        "i1",
        IntentType::SwitchToApp("com.known".into()),
        0.8,
        RiskLevel::Low,
        vec![make_action(
            ActionType::KeepAlive,
            Some("com.known"),
            ActionUrgency::Immediate,
        )],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch_with_context(&batch, &capability, &ctx);

    assert!(decisions[0].approved);
    assert_eq!(decisions[0].approved_actions.len(), 1);
    assert!(decisions[0].action_denials.is_empty());
}

#[test]
fn test_target_not_in_context_denied() {
    let engine = PolicyEngine::default();
    let capability = CapabilityLevel::for_route(DecisionRoute::CloudLlm);
    let ctx = ctx_with_packages(&["com.known"]);
    let intent = make_intent(
        "i1",
        IntentType::SwitchToApp("com.unseen".into()),
        0.8,
        RiskLevel::Low,
        vec![make_action(
            ActionType::KeepAlive,
            Some("com.unseen"),
            ActionUrgency::Immediate,
        )],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch_with_context(&batch, &capability, &ctx);

    // intent itself stays approved (no risk/conf rejection), but the action is
    // denied for hallucinated target.
    assert!(decisions[0].approved);
    assert!(decisions[0].approved_actions.is_empty());
    assert_eq!(decisions[0].action_denials.len(), 1);
    assert_eq!(
        decisions[0].action_denials[0],
        DenialReason::TargetNotInContext
    );
}

#[test]
fn test_noop_target_irrelevant() {
    // NoOp doesn't care about target — even an empty context should approve.
    let engine = PolicyEngine::default();
    let capability = CapabilityLevel::for_route(DecisionRoute::FallbackNoOp);
    let ctx = ctx_with_packages(&[]);
    let intent = make_intent(
        "i1",
        IntentType::Idle,
        0.8,
        RiskLevel::Low,
        vec![make_action(
            ActionType::NoOp,
            None,
            ActionUrgency::Immediate,
        )],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch_with_context(&batch, &capability, &ctx);

    assert!(decisions[0].approved);
    assert_eq!(decisions[0].approved_actions.len(), 1);
    assert!(decisions[0].action_denials.is_empty());
}

#[test]
fn test_prewarm_without_target_denied() {
    // PreWarmProcess with target=None must be denied even if the backend
    // capability would otherwise allow PreWarmProcess.
    let engine = PolicyEngine::default();
    let capability = CapabilityLevel::for_route(DecisionRoute::CloudLlm);
    let ctx = ctx_with_packages(&["com.known"]);
    let intent = make_intent(
        "i1",
        IntentType::OpenApp("com.known".into()),
        0.8,
        RiskLevel::Low,
        vec![make_action(
            ActionType::PreWarmProcess,
            None,
            ActionUrgency::Immediate,
        )],
    );
    let batch = make_batch(vec![intent]);
    let decisions = engine.evaluate_batch_with_context(&batch, &capability, &ctx);

    assert!(decisions[0].approved);
    assert!(decisions[0].approved_actions.is_empty());
    assert_eq!(decisions[0].action_denials.len(), 1);
    assert_eq!(
        decisions[0].action_denials[0],
        DenialReason::TargetNotInContext
    );
}
