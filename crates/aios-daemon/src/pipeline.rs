//! 上下文窗口处理管线。
//!
//! 单个窗口的处理流程: DecisionRouter → PolicyEngine → ActionExecutor。

use aios_action::DefaultActionExecutor;
use aios_agent::DecisionRouter;
use aios_collector::collection_stats::RawEventStats;
use aios_core::policy_engine::PolicyEngine;
use aios_spec::traits::ActionExecutor;
use aios_spec::{CapabilityLevel, IngestedRawEvent};

/// 处理一个上下文窗口: decision router → validate → execute
pub(crate) fn process_window(
    ctx: &aios_spec::StructuredContext,
    router: &DecisionRouter,
    policy: &PolicyEngine,
    executor: &DefaultActionExecutor,
    raw_stats: &RawEventStats,
) {
    tracing::info!(
        window_id = %ctx.window_id,
        event_count = ctx.events.len(),
        raw_event_total = raw_stats.total(),
        raw_event_stats = %raw_stats.summary_line(),
        duration_secs = ctx.duration_secs,
        "window closed, sending to agent"
    );

    let decision_result = router.evaluate(ctx);
    tracing::info!(
        route = ?decision_result.route,
        model = %decision_result.intent_batch.model,
        latency_us = decision_result.latency_us,
        error = ?decision_result.error,
        "decision backend completed"
    );

    let capability = CapabilityLevel::for_route(decision_result.route);
    let decisions =
        policy.evaluate_batch_with_context(&decision_result.intent_batch, &capability, ctx);

    let mut executed = 0u32;
    for decision in &decisions {
        if decision.approved {
            let results = executor.execute_batch(&decision.approved_actions);
            executed += results.len() as u32;
            for result in &results {
                if !result.success {
                    tracing::warn!(
                        action = %result.action_type,
                        error = ?result.error,
                        "action execution failed"
                    );
                }
            }
        } else {
            tracing::debug!(
                intent_id = %decision.intent_id,
                reason = ?decision.rejection_reason,
                "intent rejected by policy"
            );
        }
        for denial in &decision.action_denials {
            tracing::warn!(
                intent_id = %decision.intent_id,
                reason = ?denial,
                "action denied by policy"
            );
        }
    }

    tracing::info!(
        window_id = %ctx.window_id,
        intents_total = decisions.len(),
        actions_executed = executed,
        "window processed"
    );
}

// ============================================================
// Processing event dispatch
// ============================================================

#[derive(Debug)]
pub enum ProcessingEvent {
    Raw(IngestedRawEvent),
    RawChannelClosed,
    WindowExpired,
}

pub fn should_stop_processing(event: &ProcessingEvent) -> bool {
    matches!(event, ProcessingEvent::RawChannelClosed)
}
