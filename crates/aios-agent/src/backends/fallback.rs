//! FallbackNoOpBackend —— 熔断后的系统级安全兜底（采用 issue #10 方案 A）。
//!
//! 方案 A 语义：fallback 生成一个可审计的安全 NoOp，允许通过 PolicyEngine。
//! 返回的 Idle/NoOp 意图固定 `confidence=1.0`、`risk_level=Low`，确保不会被
//! PolicyEngine 的普通 confidence threshold（默认 0.3）或风险上限拦截。
//! 后端层面的失败信号（如“circuit breaker engaged”）由
//! `DecisionBackendResult::error` 单独承载，并随 audit record 的
//! `backend_error` 字段进入 trace，与意图置信度解耦。
//!
//! 这与方案 B（fallback 不进入 action pipeline）不同：方案 A 保证每条动作
//! 仍产出恰好一条终态审计记录，维持 lifecycle 不变量。

use std::time::Instant;

use aios_spec::{
    ActionType, ActionUrgency, DecisionBackendResult, DecisionRoute, Intent, IntentBatch,
    IntentType, RiskLevel, StructuredContext, SuggestedAction,
};

use crate::{new_id, DecisionBackend};

pub struct FallbackNoOpBackend;

impl DecisionBackend for FallbackNoOpBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        let start = Instant::now();
        let intent_batch = IntentBatch {
            window_id: context.window_id.clone(),
            intents: vec![Intent {
                intent_id: new_id(),
                intent_type: IntentType::Idle,
                confidence: 1.0,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::NoOp,
                    target: None,
                    urgency: ActionUrgency::IdleTime,
                }],
                rationale_tags: vec!["fallback_noop".into()],
            }],
            generated_at_ms: context.window_end_ms,
            model: "fallback-noop-v0.1".to_string(),
        };

        DecisionBackendResult {
            route: DecisionRoute::FallbackNoOp,
            intent_batch,
            rationale_tags: vec!["fallback_noop".into()],
            latency_us: start.elapsed().as_micros() as u64,
            error: Some("circuit breaker engaged — falling back to no-op".into()),
        }
    }
}
