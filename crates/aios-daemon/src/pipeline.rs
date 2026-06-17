//! 上下文窗口处理管线。
//!
//! 单个窗口的处理流程: DecisionRouter → PolicyEngine → ActionExecutor。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use aios_action::DefaultActionExecutor;
use aios_agent::DecisionRouter;
use aios_collector::collection_stats::RawEventStats;
use aios_core::policy_engine::PolicyEngine;
use aios_spec::traits::ActionExecutor;
use aios_spec::{CapabilityLevel, IngestedRawEvent};
use serde_json::json;

/// Append-only NDJSON recorder for daemon window processing.
pub struct RuntimeTraceRecorder {
    file: File,
}

impl RuntimeTraceRecorder {
    pub fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { file })
    }

    pub fn record_window(&mut self, record: &serde_json::Value) -> std::io::Result<()> {
        serde_json::to_writer(&mut self.file, record)?;
        self.file.write_all(b"\n")?;
        self.file.flush()
    }
}

/// 处理一个上下文窗口: decision router → validate → execute
pub(crate) fn process_window(
    ctx: &aios_spec::StructuredContext,
    router: &DecisionRouter,
    policy: &PolicyEngine,
    executor: &DefaultActionExecutor,
    raw_stats: &RawEventStats,
    trace_recorder: Option<&mut RuntimeTraceRecorder>,
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
    let mut execution_records = Vec::new();
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
                execution_records.push(json!({
                    "intent_id": decision.intent_id,
                    "action_type": result.action_type,
                    "target": result.target,
                    "success": result.success,
                    "error": result.error,
                    "latency_us": result.latency_us,
                }));
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

    if let Some(recorder) = trace_recorder {
        let record = json!({
            "stage": "daemon_window",
            "window_id": ctx.window_id,
            "window_start_ms": ctx.window_start_ms,
            "window_end_ms": ctx.window_end_ms,
            "duration_secs": ctx.duration_secs,
            "event_count": ctx.events.len(),
            "raw_event_total": raw_stats.total(),
            "raw_event_stats": raw_stats.summary_fields(),
            "context_summary": ctx.summary,
            "decision": {
                "route": format!("{:?}", decision_result.route),
                "model": decision_result.intent_batch.model,
                "intent_count": decision_result.intent_batch.intents.len(),
                "rationale_tags": decision_result.rationale_tags,
                "latency_us": decision_result.latency_us,
                "error": decision_result.error,
            },
            "policy": decisions.iter().map(|decision| {
                json!({
                    "intent_id": decision.intent_id,
                    "approved": decision.approved,
                    "rejection_reason": decision.rejection_reason,
                    "action_denials": decision.action_denials,
                    "approved_actions": decision.approved_actions,
                })
            }).collect::<Vec<_>>(),
            "execution": execution_records,
        });
        if let Err(error) = recorder.record_window(&record) {
            tracing::warn!(error = %error, "failed to write daemon runtime trace");
        }
    }
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
