//! Adapter 执行集成测试
//!
//! `AuthorizedAction` 的构造器是 `aios-core` 的 `pub(crate)`，外部 crate 无法直接
//! 伪造。本测试通过 `ActionLifecycle` 走完整 pipeline 来驱动真实 adapter，验证
//! `DefaultActionExecutor` 与 `OfflineAdapter` 的执行语义。

use aios_action::{DefaultActionExecutor, OfflineAdapter};
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::policy_engine::PolicyEngine;
use aios_spec::context::ContextSummary;
use aios_spec::governance::ActionState;
use aios_spec::intent::{
    ActionType, ActionUrgency, CapabilityLevel, DecisionRoute, Intent, IntentBatch, IntentType,
    RiskLevel, SuggestedAction,
};
use aios_spec::{SourceTier, StructuredContext};

fn noop_intent() -> Intent {
    Intent {
        intent_id: "intent-1".into(),
        intent_type: IntentType::Idle,
        confidence: 0.9,
        risk_level: RiskLevel::Low,
        suggested_actions: vec![SuggestedAction {
            action_type: ActionType::NoOp,
            target: None,
            urgency: ActionUrgency::Immediate,
        }],
        rationale_tags: vec![],
    }
}

fn batch_with_single(intent: Intent) -> IntentBatch {
    IntentBatch {
        window_id: "w1".into(),
        intents: vec![intent],
        generated_at_ms: 1000,
        model: "test".into(),
    }
}

fn empty_context() -> StructuredContext {
    StructuredContext {
        window_id: "w1".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events: vec![],
        summary: ContextSummary {
            foreground_apps: vec![],
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    }
}

fn permissive_capability() -> CapabilityLevel {
    CapabilityLevel {
        max_risk: RiskLevel::High,
        allowed_actions: vec![
            ActionType::NoOp,
            ActionType::PreWarmProcess,
            ActionType::PrefetchFile,
            ActionType::KeepAlive,
            ActionType::ReleaseMemory,
        ],
    }
}

fn context_with_apps(apps: &[&str]) -> StructuredContext {
    StructuredContext {
        window_id: "w1".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events: vec![],
        summary: ContextSummary {
            foreground_apps: apps.iter().map(|s| s.to_string()).collect(),
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    }
}

#[test]
fn default_executor_noop_succeeds() {
    let policy = PolicyEngine::default();
    let executor = DefaultActionExecutor::new();
    let lifecycle = ActionLifecycle::new(&policy, &executor);

    let records = lifecycle.run(
        0,
        &batch_with_single(noop_intent()),
        DecisionRoute::RuleBased,
        None,
        &permissive_capability(),
        &empty_context(),
    );

    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert!(matches!(r.terminal, ActionState::Succeeded));
    assert_eq!(r.outcome.as_ref().unwrap().summary, "noop");
}

#[test]
fn offline_adapter_covers_all_action_types() {
    let policy = PolicyEngine::default();
    let adapter = OfflineAdapter;
    let lifecycle = ActionLifecycle::new(&policy, &adapter);

    let cases = vec![
        (ActionType::NoOp, None),
        (ActionType::PreWarmProcess, Some("com.example.app")),
        (ActionType::PrefetchFile, None),
        (ActionType::KeepAlive, Some("com.example.app")),
        (ActionType::ReleaseMemory, None),
    ];

    for (action_type, target) in cases {
        let action_type_name = format!("{:?}", action_type);
        let intent = Intent {
            intent_id: "intent-1".into(),
            intent_type: IntentType::Idle,
            confidence: 0.9,
            risk_level: RiskLevel::Low,
            suggested_actions: vec![SuggestedAction {
                action_type: action_type.clone(),
                target: target.map(|s| s.to_string()),
                urgency: ActionUrgency::Immediate,
            }],
            rationale_tags: vec![],
        };

        let records = lifecycle.run(
            0,
            &batch_with_single(intent),
            DecisionRoute::RuleBased,
            None,
            &permissive_capability(),
            &context_with_apps(&["com.example.app"]),
        );

        assert_eq!(records.len(), 1, "action_type={action_type_name}");
        let r = &records[0];
        assert!(
            matches!(r.terminal, ActionState::Succeeded),
            "action_type={action_type_name} should succeed, got {:?}",
            r.terminal
        );
        assert!(
            !r.outcome.as_ref().unwrap().summary.is_empty(),
            "action_type={action_type_name} should produce non-empty summary"
        );
    }
}

#[test]
fn offline_adapter_outcome_is_deterministic() {
    let policy = PolicyEngine::default();
    let adapter = OfflineAdapter;
    let lifecycle = ActionLifecycle::new(&policy, &adapter);

    let intent = Intent {
        intent_id: "intent-1".into(),
        intent_type: IntentType::Idle,
        confidence: 0.9,
        risk_level: RiskLevel::Low,
        suggested_actions: vec![SuggestedAction {
            action_type: ActionType::PrefetchFile,
            target: Some("url:https://example.test/feed.json".into()),
            urgency: ActionUrgency::Immediate,
        }],
        rationale_tags: vec![],
    };

    let a = lifecycle.run(
        0,
        &batch_with_single(intent.clone()),
        DecisionRoute::RuleBased,
        None,
        &permissive_capability(),
        &empty_context(),
    );
    let b = lifecycle.run(
        0,
        &batch_with_single(intent),
        DecisionRoute::RuleBased,
        None,
        &permissive_capability(),
        &empty_context(),
    );

    assert_eq!(a[0].outcome, b[0].outcome);
}
