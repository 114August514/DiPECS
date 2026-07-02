//! 治理拦截率 baseline：对比默认 PolicyEngine 与策略大门完全敞开时的动作审批结果。
//!
//! 说明：DiPECS 架构中 `AuthorizedAction` 只能由 `ActionLifecycle` 在 `PolicyEngine`
//! 审查通过后构造，外部无法伪造。因此“无 PolicyEngine”场景不能直接绕过策略引擎，
//! 而是通过 `PolicyConfig { max_auto_risk: RiskLevel::High, ..Default::default() }`
//! 把策略大门打开来模拟。这样仍证明了：如果策略不拦截，同一条 High risk / 未知 target
//! 的动作会被提交到执行器。

use std::cell::RefCell;

use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::governance::{ActionAdapter, AuthorizedAction};
use aios_core::policy_engine::{PolicyConfig, PolicyEngine};
use aios_spec::governance::{ActionOutcome, ActionState, AdapterError, PolicyVerdict};
use aios_spec::{
    ActionType, ActionUrgency, CapabilityLevel, ContextSummary, DecisionRoute, DenialReason,
    Intent, IntentBatch, IntentType, RiskLevel, SanitizedEvent, SanitizedEventType, SourceTier,
    StructuredContext, SuggestedAction,
};

fn make_context_with_unknown_target() -> StructuredContext {
    StructuredContext {
        window_id: "w1".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events: vec![SanitizedEvent {
            event_id: "e1".into(),
            timestamp_ms: 500,
            event_type: SanitizedEventType::AppTransition {
                package_name: "com.example.known".into(),
                activity_class: None,
                transition: aios_spec::AppTransition::Foreground,
            },
            source_tier: SourceTier::PublicApi,
            app_package: Some("com.example.known".into()),
            uid: None,
        }],
        summary: ContextSummary {
            foreground_apps: vec!["com.example.known".into()],
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    }
}

fn make_batch_with_high_risk_unknown_target() -> IntentBatch {
    IntentBatch {
        window_id: "w1".into(),
        intents: vec![Intent {
            intent_id: "i1".into(),
            intent_type: IntentType::OpenApp("com.example.unknown".into()),
            confidence: 0.9,
            risk_level: RiskLevel::High,
            suggested_actions: vec![SuggestedAction {
                action_type: ActionType::PreWarmProcess,
                target: Some("pkg:com.example.unknown".into()),
                urgency: ActionUrgency::Immediate,
            }],
            rationale_tags: vec!["test".into()],
        }],
        generated_at_ms: 1000,
        model: "test".into(),
    }
}

/// Mock executor that records every `AuthorizedAction` it receives.
struct RecordingAdapter {
    executed: RefCell<Vec<(ActionType, Option<String>)>>,
}

impl RecordingAdapter {
    fn new() -> Self {
        Self {
            executed: RefCell::new(Vec::new()),
        }
    }
}

impl ActionAdapter for RecordingAdapter {
    fn name(&self) -> &'static str {
        "recording"
    }

    fn execute(&self, authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError> {
        let action = authorized.action();
        self.executed
            .borrow_mut()
            .push((action.action_type.clone(), action.target.clone()));
        Ok(ActionOutcome {
            action_type: format!("{:?}", action.action_type),
            target: action.target.clone(),
            summary: "executed".into(),
            latency_us: 0,
        })
    }
}

fn assert_high_risk_unknown_target_is_denied(route: DecisionRoute) {
    let policy = PolicyEngine::default();
    let batch = make_batch_with_high_risk_unknown_target();
    let ctx = make_context_with_unknown_target();
    let decisions =
        policy.evaluate_batch_with_context(&batch, &CapabilityLevel::for_route(route), &ctx);

    assert_eq!(decisions.len(), 1);
    let denied_count = decisions
        .iter()
        .filter(|d| matches!(d.verdict, PolicyVerdict::Denied(_)))
        .count();
    assert_eq!(denied_count, decisions.len(), "denial rate should be 100%");
    assert!(
        matches!(
            decisions[0].verdict,
            PolicyVerdict::Denied(DenialReason::RiskExceedsCapability)
        ),
        "expected RiskExceedsCapability, got {:?}",
        decisions[0].verdict
    );
}

#[test]
fn cloud_llm_blocks_high_risk_unknown_target() {
    assert_high_risk_unknown_target_is_denied(DecisionRoute::CloudLlm);
}

#[test]
fn local_evaluator_also_blocks_high_risk_unknown_target() {
    assert_high_risk_unknown_target_is_denied(DecisionRoute::LocalEvaluator);
}

#[test]
fn high_risk_action_executes_when_policy_gate_is_open() {
    // 用宽松的 PolicyConfig 把策略大门完全敞开（max_auto_risk = High），模拟
    // PolicyEngine 不拦截的场景。由于 `AuthorizedAction` 无法从外部构造，仍通过
    // ActionLifecycle 驱动，但把目标 package 也加入上下文以绕过 target-in-context 校验。
    let permissive_config = PolicyConfig {
        max_auto_risk: RiskLevel::High,
        ..Default::default()
    };
    let policy = PolicyEngine::new(permissive_config);
    let adapter = RecordingAdapter::new();
    let lifecycle = ActionLifecycle::new(&policy, &adapter);

    let batch = make_batch_with_high_risk_unknown_target();
    let mut ctx = make_context_with_unknown_target();
    // 把未知包名加入上下文，使 target 校验通过，从而单独验证动作会被执行器接收。
    ctx.summary
        .foreground_apps
        .push("com.example.unknown".into());

    let capability = CapabilityLevel {
        max_risk: RiskLevel::High,
        allowed_actions: vec![ActionType::PreWarmProcess],
    };

    let records = lifecycle.run(0, &batch, DecisionRoute::CloudLlm, None, &capability, &ctx);

    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert!(
        matches!(record.terminal, ActionState::Succeeded),
        "with policy gate open the action should execute, got {:?}",
        record.terminal
    );
    assert_eq!(record.denial_reason, None);

    let executed = adapter.executed.borrow();
    assert_eq!(executed.len(), 1);
    assert_eq!(executed[0].0, ActionType::PreWarmProcess);
    assert_eq!(executed[0].1.as_deref(), Some("pkg:com.example.unknown"));
}
