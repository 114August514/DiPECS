//! Context window processing pipeline.
//!
//! A closed window flows through DecisionRouter, ActionLifecycle, audit, and
//! model memory feedback.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use aios_agent::{DecisionRouter, ProfileSummarizer};
use aios_collector::collection_stats::RawEventStats;
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::context_memory::{ModelMemoryConfig, ModelMemoryStore};
use aios_spec::IngestedRawEvent;
use aios_spec::{CapabilityLevel, RecentDecisionRecord, UserBehaviorProfile};
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

type SummarizerFn = Arc<
    dyn Fn(&UserBehaviorProfile, &[RecentDecisionRecord]) -> Result<String, String> + Send + Sync,
>;

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
    summarizer_fn: Option<SummarizerFn>,
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
            summarizer_fn: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_summarizer_fn(
        interval_windows: u32,
        f: impl Fn(&UserBehaviorProfile, &[RecentDecisionRecord]) -> Result<String, String>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            summarizer: None,
            interval_windows,
            pending: None,
            last_started_window: 0,
            summarizer_fn: Some(Arc::new(f)),
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
        if self.summarizer_fn.is_none() && self.summarizer.is_none() {
            return;
        }
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

        if let Some(f) = self.summarizer_fn.clone() {
            self.pending = Some(thread::spawn(move || {
                let result = f(&profile, &recent);
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
            return;
        }

        let Some(summarizer) = self.summarizer.clone() else {
            return;
        };
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

// ============================================================
// Unit tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::{SystemTime, UNIX_EPOCH};

    use aios_agent::DecisionRouter;
    use aios_collector::collection_stats::RawEventStats;
    use aios_core::action_lifecycle::ActionLifecycle;
    use aios_core::context_memory::{ModelMemoryConfig, ModelMemoryStore};
    use aios_core::governance::{ActionAdapter, AuthorizedAction};
    use aios_core::policy_engine::PolicyEngine;
    use aios_spec::governance::{ActionOutcome, AdapterError};
    use aios_spec::{
        ContextSummary, DecisionBackendResult, DecisionRoute, IntentBatch, SourceTier,
        StructuredContext,
    };

    // ── helpers ──────────────────────────────────────────────────

    fn empty_context(window_id: &str) -> StructuredContext {
        StructuredContext {
            window_id: window_id.into(),
            window_start_ms: 0,
            window_end_ms: 10000,
            duration_secs: 10,
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

    fn empty_decision(window_id: &str) -> DecisionBackendResult {
        DecisionBackendResult {
            route: DecisionRoute::RuleBased,
            intent_batch: IntentBatch {
                window_id: window_id.into(),
                intents: vec![],
                generated_at_ms: 0,
                model: "test".into(),
            },
            rationale_tags: vec![],
            latency_us: 0,
            error: None,
        }
    }

    fn advance_memory_to_windows(memory: &mut ModelMemoryStore, n: u32) {
        for i in 0..n {
            let wid = format!("w{}", i);
            let ctx = empty_context(&wid);
            let decision = empty_decision(&wid);
            memory.observe_window(&ctx, &decision, &[]);
        }
    }

    // ── mock adapter ────────────────────────────────────────────

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

    // ============================================================
    // ProfileSummaryWorker tests
    // ============================================================

    #[test]
    fn worker_with_no_summarizer_never_starts() {
        let mut worker = ProfileSummaryWorker::new(None, 10);
        let mut memory = ModelMemoryStore::new(5);
        advance_memory_to_windows(&mut memory, 10);

        worker.maybe_start(&memory);
        assert!(worker.pending.is_none());
        // poll should also no-op; no llm_summary set
        worker.poll(&mut memory);
        let summary = memory.behavior_profile().summary;
        assert!(
            !summary.contains("llm_summary="),
            "expected no llm_summary in profile, got: {summary}"
        );
    }

    #[test]
    fn poll_noops_when_no_pending_job() {
        let mut worker = ProfileSummaryWorker::new(None, 10);
        let mut memory = ModelMemoryStore::new(5);

        worker.poll(&mut memory);
        let summary = memory.behavior_profile().summary;
        assert!(
            !summary.contains("llm_summary="),
            "expected no llm_summary in profile, got: {summary}"
        );
    }

    #[test]
    fn maybe_start_skips_before_interval() {
        let mut worker = ProfileSummaryWorker::with_summarizer_fn(5, |_, _| Ok("hello".into()));
        let mut memory = ModelMemoryStore::new(5);
        // 3 windows, interval is 5
        advance_memory_to_windows(&mut memory, 3);

        worker.maybe_start(&memory);
        assert!(worker.pending.is_none());
    }

    #[test]
    fn maybe_start_spawns_when_interval_met() {
        let mut worker = ProfileSummaryWorker::with_summarizer_fn(5, |_, _| Ok("hello".into()));
        let mut memory = ModelMemoryStore::new(5);
        advance_memory_to_windows(&mut memory, 25); // multiple of 5

        worker.maybe_start(&memory);
        assert!(worker.pending.is_some());
    }

    #[test]
    fn maybe_start_skips_when_pending_exists() {
        let mut worker = ProfileSummaryWorker::with_summarizer_fn(5, |_, _| Ok("hello".into()));
        let mut memory = ModelMemoryStore::new(5);
        advance_memory_to_windows(&mut memory, 10);

        // first call spawns
        worker.maybe_start(&memory);
        assert!(worker.pending.is_some());
        // second call skips because pending is still Some
        worker.maybe_start(&memory);
        assert!(worker.pending.is_some());
    }

    #[test]
    fn maybe_start_skips_same_last_started_window() {
        let mut worker = ProfileSummaryWorker::with_summarizer_fn(5, |_, _| Ok("hello".into()));
        let mut memory = ModelMemoryStore::new(5);
        advance_memory_to_windows(&mut memory, 10);

        // spawn and complete
        worker.maybe_start(&memory);
        // give the thread a moment to finish
        std::thread::sleep(std::time::Duration::from_millis(10));
        worker.poll(&mut memory);

        // observation_windows is still 10, same as last_started_window
        worker.maybe_start(&memory);
        assert!(worker.pending.is_none());
    }

    #[test]
    fn poll_collects_finished_job_and_sets_summary() {
        let mut worker = ProfileSummaryWorker::with_summarizer_fn(5, |_, _| {
            Ok("compressed behavior profile".into())
        });
        let mut memory = ModelMemoryStore::new(5);
        advance_memory_to_windows(&mut memory, 10);

        worker.maybe_start(&memory);
        // wait for the thread to complete
        std::thread::sleep(std::time::Duration::from_millis(20));
        worker.poll(&mut memory);

        let summary = memory.behavior_profile().summary;
        assert!(
            summary.starts_with("llm_summary=compressed behavior profile;"),
            "expected llm summary in profile, got: {summary}"
        );
        assert!(worker.pending.is_none());
        assert_eq!(worker.last_started_window, 10);
    }

    #[test]
    fn poll_handles_failed_summarization_gracefully() {
        let mut worker =
            ProfileSummaryWorker::with_summarizer_fn(5, |_, _| Err("summarization failed".into()));
        let mut memory = ModelMemoryStore::new(5);
        advance_memory_to_windows(&mut memory, 10);

        worker.maybe_start(&memory);
        std::thread::sleep(std::time::Duration::from_millis(20));
        worker.poll(&mut memory);

        // llm_summary should not be set after a failed summarization
        let summary = memory.behavior_profile().summary;
        assert!(
            !summary.contains("llm_summary="),
            "expected no llm_summary after failure, got: {summary}"
        );
        assert!(worker.pending.is_none());
    }

    // ============================================================
    // process_window tests
    // ============================================================

    fn make_deps<'a>(
        router: &'a DecisionRouter,
        lifecycle: &'a ActionLifecycle<'a>,
        memory: &'a mut ModelMemoryStore,
        memory_config: &'a ModelMemoryConfig,
        trace_recorder: Option<&'a mut RuntimeTraceRecorder>,
    ) -> WindowProcessingDeps<'a> {
        WindowProcessingDeps {
            router,
            lifecycle,
            memory,
            memory_config,
            profile_summary_worker: None,
            trace_recorder,
        }
    }

    #[test]
    fn process_window_increments_observation_windows() {
        let router = DecisionRouter::default();
        let policy = PolicyEngine::default();
        let adapter = OkAdapter;
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let mut memory = ModelMemoryStore::new(5);
        let config = ModelMemoryConfig::default();
        let stats = RawEventStats::default();
        let ctx = empty_context("w-test");

        let before = memory.observation_windows();
        let mut deps = make_deps(&router, &lifecycle, &mut memory, &config, None);
        process_window(0, &ctx, &stats, &mut deps);
        assert_eq!(memory.observation_windows(), before + 1);
    }

    #[test]
    fn process_window_handles_empty_context_without_panic() {
        let router = DecisionRouter::default();
        let policy = PolicyEngine::default();
        let adapter = OkAdapter;
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let mut memory = ModelMemoryStore::new(5);
        let config = ModelMemoryConfig::default();
        let stats = RawEventStats::default();
        let ctx = empty_context("w-empty");

        let mut deps = make_deps(&router, &lifecycle, &mut memory, &config, None);
        process_window(0, &ctx, &stats, &mut deps);
        // no panic
    }

    #[test]
    fn process_window_with_failing_adapter_does_not_panic() {
        let router = DecisionRouter::default();
        let policy = PolicyEngine::default();
        let adapter = FailAdapter;
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let mut memory = ModelMemoryStore::new(5);
        let config = ModelMemoryConfig::default();
        let stats = RawEventStats::default();
        let ctx = empty_context("w-fail");

        let mut deps = make_deps(&router, &lifecycle, &mut memory, &config, None);
        process_window(0, &ctx, &stats, &mut deps);
        // no panic on adapter failure
    }

    #[test]
    fn process_window_writes_trace_record() {
        let path = std::env::temp_dir().join(format!(
            "dipecs-daemon-trace-{}.ndjson",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut recorder = RuntimeTraceRecorder::new(&path).unwrap();

        let router = DecisionRouter::default();
        let policy = PolicyEngine::default();
        let adapter = OkAdapter;
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let mut memory = ModelMemoryStore::new(5);
        let config = ModelMemoryConfig::default();
        let stats = RawEventStats::default();
        let ctx = empty_context("w-trace");

        let mut deps = make_deps(
            &router,
            &lifecycle,
            &mut memory,
            &config,
            Some(&mut recorder),
        );
        process_window(0, &ctx, &stats, &mut deps);
        drop(recorder);

        let mut content = String::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1, "expected exactly one NDJSON line");

        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["stage"], "daemon_window");
        assert_eq!(parsed["window_ordinal"], 0);
        assert_eq!(parsed["window_id"], "w-trace");
        assert!(parsed["audit"].is_array());

        let _ = std::fs::remove_file(path);
    }
}
