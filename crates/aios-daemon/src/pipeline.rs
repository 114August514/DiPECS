//! Context window processing pipeline.
//!
//! A closed window flows through DecisionRouter, ActionLifecycle, audit, and
//! model memory feedback.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aios_agent::{DecisionRouter, ProfileSummarizer};
use aios_collector::collection_stats::RawEventStats;
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::context_builder::WindowAggregator;
use aios_core::context_memory::{ModelMemoryConfig, ModelMemoryStore};
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_spec::traits::PrivacySanitizer;
use aios_spec::IngestedRawEvent;
use aios_spec::{CapabilityLevel, RecentDecisionRecord, UserBehaviorProfile};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::{sleep_until, Instant};

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

/// 处理循环的终止原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopTermination {
    /// raw 通道关闭 (采集侧所有 sender 落地) —— 退出前已 flush 最后一个窗口。
    ChannelClosed,
}

/// 处理循环收尾摘要。
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProcessingLoopSummary {
    pub(crate) windows_closed: u32,
    pub(crate) events_processed: u64,
    pub(crate) terminated_by: LoopTermination,
}

/// Task-2 处理循环: 从 raw 通道消费已贴 SourceTier 的事件, 按时间窗口聚合, 逐窗口
/// 驱动决策 → 策略 → 执行 → 审计 → trace (`process_window`)。
///
/// 终止由通道关闭驱动: 当采集侧所有 sender 落地, `recv()` 返回 `None`, 循环 flush
/// 最后一个窗口再退出。这样 shutdown 时不会丢掉最后一个未满窗口。
///
/// 窗口计时用 `tokio::time::Instant`——生产环境等价于实时, 同时让暂停时钟测试
/// (`start_paused`) 能确定性地推进到 deadline 触发关窗。
pub(crate) async fn run_processing_loop(
    mut raw_rx: mpsc::Receiver<IngestedRawEvent>,
    sanitizer: &DefaultPrivacyAirGap,
    mut window: WindowAggregator,
    window_duration: Duration,
    deps: &mut WindowProcessingDeps<'_>,
) -> ProcessingLoopSummary {
    let mut raw_stats = RawEventStats::default();
    let mut window_ordinal = 0u32;
    let mut events_processed = 0u64;
    let mut window_deadline = Instant::now() + window_duration;

    loop {
        // 刻意不设 shutdown 分支: 优雅停机靠「采集侧 drop sender → 通道关闭 → recv() 返回
        // None → flush 最后窗口再退」(见 collection::run_collection_loop)。若在此加一条收到
        // 信号即退的臂, 会重新引入本设计要避免的「丢最后一个未满窗口」bug。
        let processing_event = tokio::select! {
            maybe = raw_rx.recv() => match maybe {
                // Box only affects this local dispatch enum's size; the raw
                // event is unboxed below before normal processing.
                Some(raw) => ProcessingEvent::Raw(Box::new(raw)),
                None => ProcessingEvent::RawChannelClosed,
            },
            _ = sleep_until(window_deadline) => ProcessingEvent::WindowExpired,
        };

        if should_stop_processing(&processing_event) {
            tracing::info!("raw event channel closed, flushing remaining events");
            let window_stats = std::mem::take(&mut raw_stats);
            if let Some(ctx) = window.close(timestamp_ms()) {
                process_window(window_ordinal, &ctx, &window_stats, deps);
                window_ordinal += 1;
            }
            return ProcessingLoopSummary {
                windows_closed: window_ordinal,
                events_processed,
                terminated_by: LoopTermination::ChannelClosed,
            };
        }

        let window_expired = matches!(processing_event, ProcessingEvent::WindowExpired);

        match processing_event {
            ProcessingEvent::Raw(ingested) => {
                // Return to the owned IngestedRawEvent shape expected by the
                // stats and sanitizer code paths.
                let ingested = *ingested;
                raw_stats.record(&ingested.raw_event);
                let sanitized =
                    sanitizer.sanitize_with_tier(ingested.raw_event, ingested.source_tier);
                window.push(sanitized);
                events_processed += 1;
            },
            ProcessingEvent::RawChannelClosed => unreachable!("handled before event dispatch"),
            ProcessingEvent::WindowExpired => {},
        }

        if window_expired || Instant::now() >= window_deadline {
            let window_stats = std::mem::take(&mut raw_stats);
            if let Some(ctx) = window.close(timestamp_ms()) {
                process_window(window_ordinal, &ctx, &window_stats, deps);
                window_ordinal += 1;
            }
            window_deadline = Instant::now() + window_duration;
        }
    }
}

/// 当前 epoch 毫秒。窗口 ID / trace 时间戳用, 与决策控制流无关。
pub(crate) fn timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
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

    use aios_action::DefaultActionExecutor;
    use aios_agent::DecisionRouter;
    use aios_collector::collection_stats::RawEventStats;
    use aios_collector::AndroidJsonlIngress;
    use aios_core::action_bus::ActionBus;
    use aios_core::action_lifecycle::ActionLifecycle;
    use aios_core::collector_ingress::RustCollectorIngress;
    use aios_core::context_builder::WindowAggregator;
    use aios_core::context_memory::{ModelMemoryConfig, ModelMemoryStore};
    use aios_core::governance::{ActionAdapter, AuthorizedAction};
    use aios_core::policy_engine::PolicyEngine;
    use aios_core::privacy_airgap::DefaultPrivacyAirGap;
    use aios_spec::governance::{ActionOutcome, AdapterError};
    use aios_spec::{
        ContextSummary, DecisionBackendResult, DecisionRoute, IntentBatch, SourceTier,
        StructuredContext,
    };

    use crate::collection::{run_collection_loop, RawEventSource};
    use tokio::sync::broadcast;

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

    // ============================================================
    // run_processing_loop 端到端测试: 真实 ActionBus 双任务通道
    //
    // 用真实 mpsc 通道连起「producer 任务 (Task-1 类比) → run_processing_loop
    // (Task-2)」, 驱动真实 sanitize → 窗口聚合 → 决策 → 策略 → 执行 → 审计 → trace
    // 全链路。终止由通道关闭驱动 (producer drop sender), 确定性收尾。
    // ============================================================

    const APP_TRANSITION_LINE: &str = r#"{"eventId":"evt-1","timestampMs":1000,"source":"UsageCollector","eventType":"app_transition","rawEvent":{"AppTransition":{"timestamp_ms":1000,"package_name":"com.android.chrome","activity_class":"MainActivity","transition":"Foreground"}},"rawPayload":{}}"#;

    const SYSTEM_LOW_BATTERY_LINE: &str = r#"{"eventId":"evt-4","timestampMs":4000,"source":"CollectorForegroundService","eventType":"system_state","rawEvent":{"SystemState":{"timestamp_ms":4000,"battery_pct":8,"is_charging":false,"network":"Wifi","ringer_mode":"Normal","location_type":"Unknown","headphone_connected":false,"bluetooth_connected":false}},"rawPayload":{}}"#;

    const NOTIFICATION_FILE_LINE: &str = r#"{"eventId":"evt-2","timestampMs":2000,"source":"NotificationCollectorService","eventType":"notification_posted","rawEvent":{"NotificationPosted":{"timestamp_ms":2000,"package_name":"com.ss.android.lark","category":"msg","channel_id":"lark_im_message","raw_title":"Zhang San","raw_text":"sent a file: report.pdf","is_ongoing":false,"group_key":"conv_42","has_picture":false}},"rawPayload":{}}"#;

    /// 走真实 ingress (与 daemon 采集任务同款) 把一行 Android JSONL 变成
    /// 带 SourceTier 的 `IngestedRawEvent`, 供 producer 灌进真实通道。
    fn ingest(line: &str) -> aios_spec::IngestedRawEvent {
        let envelope = AndroidJsonlIngress::new()
            .parse_line(line)
            .expect("parse_line ok")
            .expect("line carries a raw_event");
        RustCollectorIngress
            .accept(envelope)
            .expect("ingress accepts envelope")
    }

    fn temp_trace_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "dipecs-e2e-{tag}-{}.ndjson",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    /// T1: 两条事件经真实通道进入同一大窗口, 通道关闭时 flush 一次,
    /// 产出审计并执行动作。窗口设 3600s → 只有关闭触发关窗, 与定时器无关。
    #[tokio::test]
    async fn dual_task_pipeline_processes_events_and_flushes_on_close() {
        let (raw_tx, raw_rx, _itx, _irx) = ActionBus::new(64).split();

        // producer (Task-1 类比): 发低电量 + app 切换, 发完 drop sender → 通道关闭。
        let producer = tokio::spawn(async move {
            raw_tx.send(ingest(SYSTEM_LOW_BATTERY_LINE)).await.unwrap();
            raw_tx.send(ingest(APP_TRANSITION_LINE)).await.unwrap();
        });

        let trace_path = temp_trace_path("close");
        let mut recorder = RuntimeTraceRecorder::new(&trace_path).unwrap();
        let router = DecisionRouter::default();
        let policy = PolicyEngine::default();
        let adapter = DefaultActionExecutor::new();
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let mut memory = ModelMemoryStore::new(5);
        let config = ModelMemoryConfig::default();
        let mut worker = ProfileSummaryWorker::new(None, 10);
        let sanitizer = DefaultPrivacyAirGap;
        let window = WindowAggregator::new(3600, timestamp_ms());

        let summary = {
            let mut deps = WindowProcessingDeps {
                router: &router,
                lifecycle: &lifecycle,
                memory: &mut memory,
                memory_config: &config,
                profile_summary_worker: Some(&mut worker),
                trace_recorder: Some(&mut recorder),
            };
            run_processing_loop(
                raw_rx,
                &sanitizer,
                window,
                Duration::from_secs(3600),
                &mut deps,
            )
            .await
        };
        producer.await.unwrap();
        drop(recorder);

        assert_eq!(summary.terminated_by, LoopTermination::ChannelClosed);
        assert_eq!(summary.events_processed, 2);
        assert_eq!(
            summary.windows_closed, 1,
            "两条事件在同一大窗口 → flush 一次"
        );

        let content = std::fs::read_to_string(&trace_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1, "flush 一次 → 一行 NDJSON");
        let rec: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(rec["stage"], "daemon_window");
        assert_eq!(rec["window_ordinal"], 0);
        let audit = rec["audit"].as_array().expect("audit is array");
        assert!(!audit.is_empty(), "窗口应产出审计记录");
        let succeeded = audit
            .iter()
            .filter(|r| r["terminal"] == "Succeeded")
            .count();
        assert!(
            succeeded >= 1,
            "低电量上下文应至少授权并执行一个动作, audit={audit:?}"
        );

        let _ = std::fs::remove_file(&trace_path);
    }

    /// T2: 带原文 PII 的通知经真实通道后, daemon trace 里不得出现原文,
    /// 但保留脱敏后的语义提示 → 真实通道上守住 PrivacyAirGap 边界。
    #[tokio::test]
    async fn dual_task_pipeline_scrubs_pii_across_channel() {
        let (raw_tx, raw_rx, _itx, _irx) = ActionBus::new(64).split();
        let producer = tokio::spawn(async move {
            raw_tx.send(ingest(NOTIFICATION_FILE_LINE)).await.unwrap();
        });

        let trace_path = temp_trace_path("pii");
        let mut recorder = RuntimeTraceRecorder::new(&trace_path).unwrap();
        let router = DecisionRouter::default();
        let policy = PolicyEngine::default();
        let adapter = DefaultActionExecutor::new();
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let mut memory = ModelMemoryStore::new(5);
        let config = ModelMemoryConfig::default();
        let mut worker = ProfileSummaryWorker::new(None, 10);
        let sanitizer = DefaultPrivacyAirGap;
        let window = WindowAggregator::new(3600, timestamp_ms());

        let summary = {
            let mut deps = WindowProcessingDeps {
                router: &router,
                lifecycle: &lifecycle,
                memory: &mut memory,
                memory_config: &config,
                profile_summary_worker: Some(&mut worker),
                trace_recorder: Some(&mut recorder),
            };
            run_processing_loop(
                raw_rx,
                &sanitizer,
                window,
                Duration::from_secs(3600),
                &mut deps,
            )
            .await
        };
        producer.await.unwrap();
        drop(recorder);

        assert_eq!(summary.events_processed, 1);
        assert_eq!(summary.windows_closed, 1);

        let content = std::fs::read_to_string(&trace_path).unwrap();
        for pii in ["Zhang San", "report.pdf", "sent a file"] {
            assert!(
                !content.contains(pii),
                "原文 PII '{pii}' 泄漏进 daemon trace: {content}"
            );
        }
        assert!(
            content.contains("FileMention"),
            "脱敏应保留 FileMention 语义提示: {content}"
        );

        let _ = std::fs::remove_file(&trace_path);
    }

    /// T3: 空事件流 (producer 立刻 drop sender) 应干净收尾, 不 panic, 不写 trace。
    #[tokio::test]
    async fn dual_task_pipeline_empty_stream_closes_cleanly() {
        let (raw_tx, raw_rx, _itx, _irx) = ActionBus::new(64).split();
        drop(raw_tx); // 不发任何事件, 立刻关闭通道

        let trace_path = temp_trace_path("empty");
        let mut recorder = RuntimeTraceRecorder::new(&trace_path).unwrap();
        let router = DecisionRouter::default();
        let policy = PolicyEngine::default();
        let adapter = DefaultActionExecutor::new();
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let mut memory = ModelMemoryStore::new(5);
        let config = ModelMemoryConfig::default();
        let mut worker = ProfileSummaryWorker::new(None, 10);
        let sanitizer = DefaultPrivacyAirGap;
        let window = WindowAggregator::new(3600, timestamp_ms());

        let summary = {
            let mut deps = WindowProcessingDeps {
                router: &router,
                lifecycle: &lifecycle,
                memory: &mut memory,
                memory_config: &config,
                profile_summary_worker: Some(&mut worker),
                trace_recorder: Some(&mut recorder),
            };
            run_processing_loop(
                raw_rx,
                &sanitizer,
                window,
                Duration::from_secs(3600),
                &mut deps,
            )
            .await
        };
        drop(recorder);

        assert_eq!(summary.terminated_by, LoopTermination::ChannelClosed);
        assert_eq!(summary.events_processed, 0);
        assert_eq!(summary.windows_closed, 0, "空窗口不关窗");

        let content = std::fs::read_to_string(&trace_path).unwrap_or_default();
        assert!(content.trim().is_empty(), "空流不应写 trace: {content}");

        let _ = std::fs::remove_file(&trace_path);
    }

    /// T4: 定时器驱动的多窗口轮转。暂停时钟下, 每灌一批事件后推进时钟越过窗口,
    /// 由 WindowExpired 分支关窗。`join!` 让 producer 与循环在同一任务并发,
    /// 规避 spawn 的 'static 借用限制; 推进前事件已排空 → 无 select 竞态。
    #[tokio::test(start_paused = true)]
    async fn dual_task_pipeline_rotates_windows_on_timer() {
        let (raw_tx, raw_rx, _itx, _irx) = ActionBus::new(64).split();

        let router = DecisionRouter::default();
        let policy = PolicyEngine::default();
        let adapter = DefaultActionExecutor::new();
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let mut memory = ModelMemoryStore::new(5);
        let config = ModelMemoryConfig::default();
        let mut worker = ProfileSummaryWorker::new(None, 10);
        let sanitizer = DefaultPrivacyAirGap;
        let window_dur = Duration::from_secs(10);
        let window = WindowAggregator::new(10, timestamp_ms());

        let mut deps = WindowProcessingDeps {
            router: &router,
            lifecycle: &lifecycle,
            memory: &mut memory,
            memory_config: &config,
            profile_summary_worker: Some(&mut worker),
            trace_recorder: None,
        };

        let control = async move {
            // W1: 发事件 → 越过窗口 → 定时器关 W1。
            raw_tx.send(ingest(APP_TRANSITION_LINE)).await.unwrap();
            tokio::time::sleep(Duration::from_secs(11)).await;
            // W2: 再发再越过 → 关 W2。
            raw_tx.send(ingest(SYSTEM_LOW_BATTERY_LINE)).await.unwrap();
            tokio::time::sleep(Duration::from_secs(11)).await;
            drop(raw_tx); // 通道关闭 → 循环 flush(空) + 退出。
        };

        let (summary, ()) = tokio::join!(
            run_processing_loop(raw_rx, &sanitizer, window, window_dur, &mut deps),
            control,
        );

        assert_eq!(summary.terminated_by, LoopTermination::ChannelClosed);
        assert_eq!(summary.events_processed, 2);
        assert_eq!(summary.windows_closed, 2, "两个非空窗口各由定时器关一次");
    }

    // ============================================================
    // 真·双任务端到端: 真实 Task-1 (run_collection_loop) + 真实通道 +
    // 真实 Task-2 (run_processing_loop) + 真实 shutdown 广播链。
    // 只有「事件从哪来」是替身 (MockRawEventSource): 真实采集器要读真机
    // /proc、电量、Binder, 不可确定性单测。
    // ============================================================

    /// 合成事件源: 第一次 poll 交出预置事件, 之后每 tick 返回空。
    struct MockRawEventSource {
        pending: Vec<aios_spec::IngestedRawEvent>,
    }

    impl RawEventSource for MockRawEventSource {
        fn poll(&mut self, _now_ms: i64) -> Vec<aios_spec::IngestedRawEvent> {
            std::mem::take(&mut self.pending)
        }
    }

    /// T5: 两个任务都是真的。真实采集循环把合成源的事件推进真实 `ActionBus` 通道,
    /// 真实处理循环消费; 用真实 shutdown 广播触发收尾链:
    /// broadcast → Task-1 `try_recv` → return → drop sender → 通道关闭 → Task-2 flush。
    ///
    /// 确定性来自采集循环「先 poll 后查 shutdown」+ mpsc「关闭前必先交付缓冲」:
    /// 无论 shutdown 何时触发, 首次 poll 的事件都会先入通道并被处理。
    #[tokio::test]
    async fn dual_task_full_pipeline_real_shutdown_chain() {
        let (raw_tx, raw_rx, _itx, _irx) = ActionBus::new(64).split();
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        // 真实 Task-1: run_collection_loop + 合成源 (发两条事件, 之后每 tick 空)。
        let source = MockRawEventSource {
            pending: vec![ingest(SYSTEM_LOW_BATTERY_LINE), ingest(APP_TRANSITION_LINE)],
        };
        let collect_handle = tokio::spawn(run_collection_loop(
            Box::new(source),
            raw_tx,
            shutdown_rx,
            Duration::from_millis(1),
        ));

        // 真实 Task-2 依赖。
        let trace_path = temp_trace_path("dual");
        let mut recorder = RuntimeTraceRecorder::new(&trace_path).unwrap();
        let router = DecisionRouter::default();
        let policy = PolicyEngine::default();
        let adapter = DefaultActionExecutor::new();
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let mut memory = ModelMemoryStore::new(5);
        let config = ModelMemoryConfig::default();
        let mut worker = ProfileSummaryWorker::new(None, 10);
        let sanitizer = DefaultPrivacyAirGap;
        let window = WindowAggregator::new(3600, timestamp_ms());

        // 触发真实 shutdown 链。
        let control = async {
            shutdown_tx.send(()).unwrap();
        };

        let summary = {
            let mut deps = WindowProcessingDeps {
                router: &router,
                lifecycle: &lifecycle,
                memory: &mut memory,
                memory_config: &config,
                profile_summary_worker: Some(&mut worker),
                trace_recorder: Some(&mut recorder),
            };
            let (summary, ()) = tokio::join!(
                run_processing_loop(
                    raw_rx,
                    &sanitizer,
                    window,
                    Duration::from_secs(3600),
                    &mut deps,
                ),
                control,
            );
            summary
        };
        collect_handle.await.unwrap();
        drop(recorder);

        assert_eq!(summary.terminated_by, LoopTermination::ChannelClosed);
        assert_eq!(
            summary.events_processed, 2,
            "两条采集事件穿过真实通道被处理"
        );
        assert_eq!(summary.windows_closed, 1);

        // trace 里应有真实审计记录。
        let content = std::fs::read_to_string(&trace_path).unwrap();
        assert_eq!(content.lines().count(), 1);
        let rec: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(rec["stage"], "daemon_window");
        assert!(rec["audit"].as_array().is_some_and(|a| !a.is_empty()));

        let _ = std::fs::remove_file(&trace_path);
    }
}
