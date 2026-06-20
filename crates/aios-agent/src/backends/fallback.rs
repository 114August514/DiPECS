//! FallbackNoOpBackend — circuit breaker 熔断后的最终安全后端。
//!
//! 返回单个 Idle/NoOp 意图。`confidence` 取 1.0：
//! "做什么也不做" 是确定性的安全选择，不应被 policy 的置信度门槛
//! (policy_engine 默认 0.3) 拦截。后端层面的失败信号由
//! `DecisionBackendResult::error` 单独承载，与意图置信度解耦，
//! 这样 fallback 路径产生的 NoOp 仍能流向 executor 形成完整审计链。

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
