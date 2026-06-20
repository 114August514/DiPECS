//! 上下文窗口处理管线。
//!
//! 单个窗口的处理流程: DecisionRouter → ActionLifecycle → AuditRecords。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use aios_agent::DecisionRouter;
use aios_collector::collection_stats::RawEventStats;
use aios_core::action_lifecycle::ActionLifecycle;
use aios_spec::CapabilityLevel;
use aios_spec::IngestedRawEvent;
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

/// 处理一个上下文窗口: decision router → action lifecycle → audit records
pub(crate) fn process_window(
    window_ordinal: u32,
    ctx: &aios_spec::StructuredContext,
    router: &DecisionRouter,
    lifecycle: &ActionLifecycle,
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
    let audit_records = lifecycle.run(
        window_ordinal,
        &decision_result.intent_batch,
        &capability,
        ctx,
    );

    let executed = audit_records
        .iter()
        .filter(|r| matches!(r.terminal, aios_spec::governance::ActionState::Succeeded))
        .count() as u32;
    let denied = audit_records
        .iter()
        .filter(|r| {
            matches!(
                r.terminal,
                aios_spec::governance::ActionState::RejectedInvalidSchema
                    | aios_spec::governance::ActionState::DeniedByCapability
                    | aios_spec::governance::ActionState::DeniedByPolicy
            )
        })
        .count() as u32;
    let failed = audit_records
        .iter()
        .filter(|r| matches!(r.terminal, aios_spec::governance::ActionState::Failed))
        .count() as u32;

    for record in &audit_records {
        match record.terminal {
            aios_spec::governance::ActionState::Failed => {
                tracing::warn!(
                    coord = ?record.coord,
                    action = ?record.action_type,
                    error = ?record.error,
                    "action execution failed"
                );
            },
            aios_spec::governance::ActionState::DeniedByCapability
            | aios_spec::governance::ActionState::DeniedByPolicy
            | aios_spec::governance::ActionState::RejectedInvalidSchema => {
                tracing::warn!(
                    coord = ?record.coord,
                    action = ?record.action_type,
                    reason = ?record.denial_reason,
                    "action denied"
                );
            },
            _ => {},
        }
    }

    tracing::info!(
        window_id = %ctx.window_id,
        intents_total = decision_result.intent_batch.intents.len(),
        actions_executed = executed,
        actions_denied = denied,
        actions_failed = failed,
        "window processed"
    );

    if let Some(recorder) = trace_recorder {
        let record = json!({
            "stage": "daemon_window",
            "window_ordinal": window_ordinal,
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
            "audit": audit_records,
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
