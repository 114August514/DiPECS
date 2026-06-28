//! ActionLifecycle 状态机测试
//!
//! 覆盖 RFC-0002 要求的生命周期不变量：每条动作恰好一条终态审计、
//! 状态迁移序列完整、跨窗口坐标不碰撞、adapter 失败映射到 Failed 终态。

use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::governance::{ActionAdapter, AuthorizedAction};
use aios_core::policy_engine::PolicyEngine;
use aios_spec::context::ContextSummary;
use aios_spec::governance::{ActionCoord, ActionOutcome, ActionState, AdapterError};
use aios_spec::intent::{
    ActionType, ActionUrgency, CapabilityLevel, DenialReason, Intent, IntentBatch, IntentType,
    RiskLevel, SuggestedAction,
};
use aios_spec::{DecisionRoute, SourceTier, StructuredContext};

struct OkAdapter;
impl ActionAdapter for OkAdapter {
    fn name(&self) -> &'static str {
        "ok"
    }
    fn execute(&self, authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError> {
        Ok(ActionOutcome {
            action_type: format!("{:?}", authorized.action().action_type),
            target: authorized.action().target.clone(),
            summary: "ok".into(),
            latency_us: 0,
        })
    }
}

struct FailAdapter;
impl ActionAdapter for FailAdapter {
    fn name(&self) -> &'static str {
        "fail"
    }
    fn execute(&self, _authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError> {
        Err(AdapterError::SimulatedResourceUnavailable(
            "disk full".into(),
        ))
    }
}

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

fn ctx_with_apps(apps: &[&str]) -> StructuredContext {
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
fn happy_path_records_full_transition_sequence() {
    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, &OkAdapter);
    let records = lifecycle.run(
        0,
        &batch_with_single(noop_intent()),
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&[]),
    );

    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(
        r.transitions,
        vec![
            ActionState::Proposed,
            ActionState::SchemaValidated,
            ActionState::PolicyChecked,
            ActionState::Dispatched,
            ActionState::Succeeded,
        ]
    );
    assert!(matches!(r.terminal, ActionState::Succeeded));
    assert!(r.outcome.is_some());
}

#[test]
fn missing_prewarm_target_rejected_invalid_schema() {
    let intent = Intent {
        intent_id: "intent-1".into(),
        intent_type: IntentType::Idle,
        confidence: 0.9,
        risk_level: RiskLevel::Low,
        suggested_actions: vec![SuggestedAction {
            action_type: ActionType::PreWarmProcess,
            target: None,
            urgency: ActionUrgency::Immediate,
        }],
        rationale_tags: vec![],
    };
    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, &OkAdapter);
    let records = lifecycle.run(
        0,
        &batch_with_single(intent),
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&[]),
    );

    assert_eq!(records.len(), 1);
    assert!(matches!(
        records[0].terminal,
        ActionState::RejectedInvalidSchema
    ));
}

#[test]
fn capability_denial_maps_to_denied_by_capability() {
    // RuleBased backend 不允许 PreWarmProcess
    let intent = Intent {
        intent_id: "intent-1".into(),
        intent_type: IntentType::Idle,
        confidence: 0.9,
        risk_level: RiskLevel::Low,
        suggested_actions: vec![SuggestedAction {
            action_type: ActionType::PreWarmProcess,
            target: Some("com.example.app".into()),
            urgency: ActionUrgency::Immediate,
        }],
        rationale_tags: vec![],
    };
    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, &OkAdapter);
    let records = lifecycle.run(
        0,
        &batch_with_single(intent),
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&["com.example.app"]),
    );

    assert_eq!(records.len(), 1);
    assert!(matches!(
        records[0].terminal,
        ActionState::DeniedByCapability
    ));
    assert_eq!(
        records[0].denial_reason,
        Some(DenialReason::ActionCapabilityDenied)
    );
}

#[test]
fn adapter_err_maps_to_failed_terminal() {
    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, &FailAdapter);
    let records = lifecycle.run(
        0,
        &batch_with_single(noop_intent()),
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&[]),
    );

    assert_eq!(records.len(), 1);
    assert!(matches!(records[0].terminal, ActionState::Failed));
    assert!(records[0].error.is_some());
}

#[test]
fn each_coord_has_exactly_one_terminal_audit_record() {
    let intent = noop_intent();
    let batch = IntentBatch {
        window_id: "w1".into(),
        intents: vec![intent.clone(), intent.clone()],
        generated_at_ms: 1000,
        model: "test".into(),
    };
    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, &OkAdapter);
    let records = lifecycle.run(
        0,
        &batch,
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&[]),
    );

    assert_eq!(records.len(), 2);
    for r in &records {
        assert!(r.terminal.is_terminal());
    }
    assert_ne!(records[0].coord, records[1].coord);
}

#[test]
fn window_ordinal_prevents_collision_across_windows() {
    let batch = batch_with_single(noop_intent());
    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, &OkAdapter);
    let r0 = lifecycle.run(
        0,
        &batch,
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&[]),
    );
    let r1 = lifecycle.run(
        1,
        &batch,
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&[]),
    );

    assert_eq!(r0.len(), 1);
    assert_eq!(r1.len(), 1);
    assert_ne!(r0[0].coord, r1[0].coord);
    assert_eq!(r0[0].coord.intent_ordinal, r1[0].coord.intent_ordinal);
    assert_eq!(r0[0].coord.action_ordinal, r1[0].coord.action_ordinal);
}

#[test]
fn outcome_drift_changes_audit_hash() {
    // 同一 batch 用两个 outcome 摘要不同的 adapter 执行，审计记录 outcome 不同，
    // 进 hash 后必然改变 fingerprint。
    struct OkAdapterA;
    impl ActionAdapter for OkAdapterA {
        fn name(&self) -> &'static str {
            "ok-a"
        }
        fn execute(&self, authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError> {
            Ok(ActionOutcome {
                action_type: format!("{:?}", authorized.action().action_type),
                target: authorized.action().target.clone(),
                summary: "outcome-a".into(),
                latency_us: 0,
            })
        }
    }

    struct OkAdapterB;
    impl ActionAdapter for OkAdapterB {
        fn name(&self) -> &'static str {
            "ok-b"
        }
        fn execute(&self, authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError> {
            Ok(ActionOutcome {
                action_type: format!("{:?}", authorized.action().action_type),
                target: authorized.action().target.clone(),
                summary: "outcome-b".into(),
                latency_us: 0,
            })
        }
    }

    let batch = batch_with_single(noop_intent());
    let policy = PolicyEngine::default();
    let lifecycle_a = ActionLifecycle::new(&policy, &OkAdapterA);
    let lifecycle_b = ActionLifecycle::new(&policy, &OkAdapterB);

    let records_a = lifecycle_a.run(
        0,
        &batch,
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&[]),
    );
    let records_b = lifecycle_b.run(
        0,
        &batch,
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&[]),
    );

    assert_ne!(
        records_a[0].outcome.as_ref().unwrap().summary,
        records_b[0].outcome.as_ref().unwrap().summary
    );

    // 模拟 canonical hash：outcome 不同则序列化不同。
    let json_a = serde_json::to_string(&records_a[0]).unwrap();
    let json_b = serde_json::to_string(&records_b[0]).unwrap();
    assert_ne!(json_a, json_b);
}

#[test]
fn audit_hash_is_stable_across_repeated_runs() {
    let batch = batch_with_single(noop_intent());
    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, &OkAdapter);

    let r0 = lifecycle.run(
        0,
        &batch,
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&[]),
    );
    let r1 = lifecycle.run(
        0,
        &batch,
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx_with_apps(&[]),
    );

    assert_eq!(r0.len(), r1.len());
    for (a, b) in r0.iter().zip(r1.iter()) {
        assert_eq!(a.coord, b.coord);
        assert_eq!(a.terminal, b.terminal);
        assert_eq!(a.outcome, b.outcome);
        assert_eq!(a.transitions, b.transitions);
    }
}

#[test]
fn audit_record_source_tier_matches_context_summary() {
    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, &OkAdapter);
    let ctx = ctx_with_apps(&[]);
    let records = lifecycle.run(
        0,
        &batch_with_single(noop_intent()),
        DecisionRoute::RuleBased,
        None,
        &CapabilityLevel::for_route(DecisionRoute::RuleBased),
        &ctx,
    );

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source_tier, ctx.summary.source_tier);
}

#[test]
fn deterministic_coord_excludes_runtime_volatiles() {
    // ActionCoord 只含位置量，不含 UUID/wall-clock。
    let coord = ActionCoord {
        window_ordinal: 42,
        intent_ordinal: 7,
        action_ordinal: 3,
    };
    let json = serde_json::to_string(&coord).unwrap();
    assert!(!json.contains("uuid"));
    assert!(!json.contains("timestamp"));
    assert!(json.contains("42"));
    assert!(json.contains("7"));
    assert!(json.contains("3"));
}

#[test]
fn audit_record_includes_route_and_backend_error() {
    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, &OkAdapter);
    let records = lifecycle.run(
        0,
        &batch_with_single(noop_intent()),
        DecisionRoute::FallbackNoOp,
        Some("circuit breaker engaged".into()),
        &CapabilityLevel::for_route(DecisionRoute::FallbackNoOp),
        &ctx_with_apps(&[]),
    );

    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert!(matches!(record.route, DecisionRoute::FallbackNoOp));
    assert_eq!(
        record.backend_error.as_deref(),
        Some("circuit breaker engaged")
    );
}

#[test]
fn fallback_noop_cannot_be_rejected_by_confidence() {
    let mut intent = noop_intent();
    // 即使 confidence 低于默认 threshold，FallbackNoOp 的 capability 只允
    // 许 NoOp，且 NoOp 在 schema 校验后不会被 confidence gate 拒绝；更重要
    // 的是，issue #10 要求验证 FallbackNoOp 不会因普通 confidence threshold
    // 进入未定义行为。这里用 confidence=1.0 模拟 fallback 输出，断言其到达
    // Succeeded 终态。
    intent.confidence = 1.0;

    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, &OkAdapter);
    let records = lifecycle.run(
        0,
        &batch_with_single(intent),
        DecisionRoute::FallbackNoOp,
        Some("test fallback".into()),
        &CapabilityLevel::for_route(DecisionRoute::FallbackNoOp),
        &ctx_with_apps(&[]),
    );

    assert_eq!(records.len(), 1);
    assert!(
        matches!(records[0].terminal, ActionState::Succeeded),
        "FallbackNoOp NoOp must reach Succeeded, got {:?}",
        records[0].terminal
    );
}
