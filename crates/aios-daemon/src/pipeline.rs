//! Context window processing pipeline.
//!
//! A closed window flows through DecisionRouter, ActionLifecycle, audit, and
//! model memory feedback.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::thread::{self, JoinHandle};

use aios_agent::{DecisionRouter, ProfileSummarizer};
use aios_collector::collection_stats::RawEventStats;
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::context_memory::{ModelMemoryConfig, ModelMemoryStore};
use aios_spec::IngestedRawEvent;
use aios_spec::{CapabilityLevel, RecentDecisionRecord};
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

/// Mutable dependencies shared while processing a closed context window.
pub(crate) struct WindowProcessingDeps<'a> {
    pub(crate) router: &'a DecisionRouter,
    pub(crate) lifecycle: &'a ActionLifecycle<'a>,
    pub(crate) memory: &'a mut ModelMemoryStore,
    pub(crate) memory_config: &'a ModelMemoryConfig,
    pub(crate) profile_summary_worker: Option<&'a mut ProfileSummaryWorker>,
    pub(crate) trace_recorder: Option<&'a mut RuntimeTraceRecorder>,
}

/// Processes one closed context window and feeds the result back into memory.
pub(crate) fn process_window(
    window_ordinal: u32,
    ctx: &aios_spec::StructuredContext,
    raw_stats: &RawEventStats,
    deps: &mut WindowProcessingDeps<'_>,
) {
    tracing::info!(
        window_id = %ctx.window_id,
        event_count = ctx.events.len(),
        raw_event_total = raw_stats.total(),
        raw_event_stats = %raw_stats.summary_line(),
        duration_secs = ctx.duration_secs,
        "window closed, sending to agent"
    );

    if let Some(worker) = &mut deps.profile_summary_worker {
        worker.poll(deps.memory);
    }

    let model_input = deps.memory.model_input(ctx);
    let decision_result = deps.router.evaluate_model_input(&model_input);
    tracing::info!(
        route = ?decision_result.route,
        model = %decision_result.intent_batch.model,
        latency_us = decision_result.latency_us,
        error = ?decision_result.error,
        "decision backend completed"
    );

    let capability = CapabilityLevel::for_route(decision_result.route);
    let audit_records = deps.lifecycle.run(
        window_ordinal,
        &decision_result.intent_batch,
        decision_result.route,
        decision_result.error.clone(),
        &capability,
        ctx,
    );

    deps.memory
        .observe_window(ctx, &decision_result, &audit_records);
    if let Some(worker) = &mut deps.profile_summary_worker {
        worker.poll(deps.memory);
        worker.maybe_start(deps.memory);
    }
    deps.memory.persist_if_configured(deps.memory_config);
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

    if let Some(recorder) = &mut deps.trace_recorder {
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
            "behavior_profile": deps.memory.behavior_profile(),
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

/// Non-blocking profile compression worker.
///
/// The decision path keeps using local counters and the last completed summary.
/// When the configured interval is reached, this worker snapshots sanitized
/// memory and asks the LLM to compress it on a background thread.
pub(crate) struct ProfileSummaryWorker {
    summarizer: Option<ProfileSummarizer>,
    interval_windows: u32,
    pending: Option<JoinHandle<ProfileSummaryJobResult>>,
    last_started_window: u32,
}

struct ProfileSummaryJobResult {
    observed_windows: u32,
    recent: Vec<RecentDecisionRecord>,
    result: Result<String, String>,
}

impl ProfileSummaryWorker {
    pub(crate) fn new(summarizer: Option<ProfileSummarizer>, interval_windows: u32) -> Self {
        Self {
            summarizer,
            interval_windows,
            pending: None,
            last_started_window: 0,
        }
    }

    pub(crate) fn poll(&mut self, memory: &mut ModelMemoryStore) {
        let Some(handle) = self.pending.take() else {
            return;
        };
        if !handle.is_finished() {
            self.pending = Some(handle);
            return;
        }

        match handle.join() {
            Ok(job) => match job.result {
                Ok(summary) => {
                    tracing::info!(
                        observed_windows = job.observed_windows,
                        recent_windows = job.recent.len(),
                        "profile summary refreshed"
                    );
                    memory.set_llm_summary(summary);
                },
                Err(error) => {
                    tracing::warn!(
                        observed_windows = job.observed_windows,
                        recent_windows = job.recent.len(),
                        error = %error,
                        "profile summary refresh failed"
                    );
                },
            },
            Err(_) => {
                tracing::warn!("profile summary worker panicked");
            },
        }
    }

    pub(crate) fn maybe_start(&mut self, memory: &ModelMemoryStore) {
        let Some(summarizer) = self.summarizer.clone() else {
            return;
        };
        if self.pending.is_some() || self.interval_windows == 0 {
            return;
        }
        let windows = memory.observation_windows();
        if windows == 0
            || !windows.is_multiple_of(self.interval_windows)
            || windows == self.last_started_window
        {
            return;
        }

        let profile = memory.behavior_profile();
        let recent = memory.recent_feedback();
        self.last_started_window = windows;
        self.pending = Some(thread::spawn(move || {
            let result = summarizer.summarize(&profile, &recent);
            ProfileSummaryJobResult {
                observed_windows: windows,
                recent,
                result,
            }
        }));
        tracing::info!(
            observed_windows = windows,
            "profile summary refresh started"
        );
    }
}
// ============================================================
// Processing event dispatch
// ============================================================

#[derive(Debug)]
pub enum ProcessingEvent {
    // Keep the enum small even when RawEvent grows additional metadata fields.
    // The processing loop immediately unboxes this before sanitization.
    Raw(Box<IngestedRawEvent>),

    RawChannelClosed,

    WindowExpired,
}

pub fn should_stop_processing(event: &ProcessingEvent) -> bool {
    matches!(event, ProcessingEvent::RawChannelClosed)
}
