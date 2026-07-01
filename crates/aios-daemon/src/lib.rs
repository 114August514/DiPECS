//! # dipecsd - DiPECS system daemon
//!
//! Deployment path: `/system/bin/dipecsd`
//! Startup: `dipecsd [--no-daemon] [--verbose]`
//!
//! ## Runtime Modes
//!
//! - daemon mode: fork to the background and keep collecting system events
//! - `--no-daemon`: foreground mode for debugging and development
//!
//! ## Data Flow
//!
//! ```text
//! Task 1 (collection): BinderProbe + ProcReader + SysCollector -> bus.raw_events_tx
//! Task 2 (processing): bus.raw_events_rx -> PrivacyAirGap -> WindowAggregator
//!                      -> DecisionRouter -> PolicyEngine -> ActionExecutor
//! ```
//!
//! ## Current Implementation Status
//!
//! - ProcReader: available on Linux/Android
//! - SystemStateCollector: available with battery/network fallback
//! - BinderProbe/FanotifyMonitor: interfaces complete; real fd/eBPF attach requires privileged deployment
//! - Decision routing: RuleBased, LocalEvaluator, CloudLlm, and FallbackNoOp
//! - Action execution: Android bridge plus local fallback behavior
use std::time::Duration;

use aios_action::{AndroidAdapter, AndroidBridgeConfig, DefaultActionExecutor};
use aios_agent::{DecisionRouter, ProfileSummarizer};
use aios_core::action_bus::ActionBus;
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::context_builder::WindowAggregator;
use aios_core::context_memory::{ModelMemoryConfig, ModelMemoryStore};
use aios_core::governance::ActionAdapter;
use aios_core::policy_engine::PolicyEngine;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;

mod collection;
mod daemon;
pub mod pipeline;

use collection::{run_collection_loop, SystemRawEventSource, BINDER_POLL_INTERVAL_MS};
use pipeline::{timestamp_ms, ProfileSummaryWorker, RuntimeTraceRecorder, WindowProcessingDeps};

/// Context window duration, in seconds.
const WINDOW_DURATION_SECS: u64 = 10;
const ANDROID_TRACE_JSONL_ENV: &str = "DIPECS_ANDROID_TRACE_JSONL";
const RUNTIME_TRACE_OUTPUT_ENV: &str = "DIPECS_RUNTIME_TRACE_OUTPUT";
const PROFILE_SUMMARY_INTERVAL_ENV: &str = "DIPECS_PROFILE_SUMMARY_INTERVAL_WINDOWS";

pub async fn run() -> anyhow::Result<()> {
    // 1. Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("dipecs=info".parse()?),
        )
        .init();

    // 2. Parse command-line arguments
    let args: Vec<String> = std::env::args().collect();
    let no_daemon = args.iter().any(|a| a == "--no-daemon");
    let android_trace_jsonl = android_trace_jsonl_path(&args);
    let runtime_trace_output = runtime_trace_output_path(&args);

    if !no_daemon {
        daemon::daemonize();
    }
    tracing::info!("dipecsd starting (no-daemon={})", no_daemon);
    if let Some(path) = &android_trace_jsonl {
        tracing::info!(
            path = %path.display(),
            "Android collector JSONL ingress enabled"
        );
    }
    if let Some(path) = &runtime_trace_output {
        tracing::info!(path = %path.display(), "daemon runtime trace enabled");
    }

    // 3. Initialize ActionBus and shutdown signals
    // split 交出 raw sender 的所有权(不在 bus 里残留副本): Task 1 收尾落地 sender 后,
    // raw 通道才会关闭, Task 2 处理循环据此 flush 最后窗口并退出。
    let (collect_raw_tx, raw_rx, _intent_tx, _intent_rx) = ActionBus::new(4096).split();
    // ---- Task 1: collection ----
    let collect_shutdown = daemon::install_signal_handlers();
    // Task 1 通过真实采集循环把事件推进 raw 通道。事件源是 SystemRawEventSource
    // (封装 proc/sys/binder/android 采集器); 收到 shutdown 即返回, sender 落地关闭通道。
    let collect_source = SystemRawEventSource::new(android_trace_jsonl);
    let collect_handle = tokio::spawn(run_collection_loop(
        Box::new(collect_source),
        collect_raw_tx,
        collect_shutdown,
        Duration::from_millis(BINDER_POLL_INTERVAL_MS),
    ));

    // ---- Task 2: processing pipeline (main task) ----
    tracing::info!("processing task started");

    let sanitizer = DefaultPrivacyAirGap;
    let router = DecisionRouter::default();
    let profile_summarizer = ProfileSummarizer::from_env()
        .map_err(|error| anyhow::anyhow!("profile summarizer configuration failed: {error}"))?;
    let mut profile_summary_worker =
        ProfileSummaryWorker::new(profile_summarizer, profile_summary_interval_windows());
    let policy = PolicyEngine::default();
    // Adapter selection at startup (not per-action): if the Android bridge is
    // configured via env, route through the on-device AndroidAdapter; otherwise
    // use the deterministic desktop stub. This is the minimal AdapterRegistry.
    let adapter: Box<dyn ActionAdapter> = match AndroidBridgeConfig::from_env() {
        Some(config) => {
            tracing::info!(
                host = %config.host,
                port = config.port,
                "Android action bridge enabled; routing through AndroidAdapter"
            );
            Box::new(AndroidAdapter::new(config))
        },
        None => Box::new(DefaultActionExecutor::new()),
    };
    let lifecycle = ActionLifecycle::new(&policy, adapter.as_ref());
    let mut trace_recorder = match runtime_trace_output {
        Some(path) => Some(RuntimeTraceRecorder::new(&path).map_err(|error| {
            anyhow::anyhow!(
                "opening daemon runtime trace output {} failed: {error}",
                path.display()
            )
        })?),
        None => None,
    };
    let window_dur = Duration::from_secs(WINDOW_DURATION_SECS);
    let window = WindowAggregator::new(WINDOW_DURATION_SECS, timestamp_ms());
    let model_memory_config = ModelMemoryConfig::from_env();
    let mut model_memory = ModelMemoryStore::load_or_default(&model_memory_config);

    // Task 2 独占 raw 通道; Task 1 收尾落地 sender → 通道关闭 → 循环 flush 最后窗口后返回。
    let mut deps = WindowProcessingDeps {
        router: &router,
        lifecycle: &lifecycle,
        memory: &mut model_memory,
        memory_config: &model_memory_config,
        profile_summary_worker: Some(&mut profile_summary_worker),
        trace_recorder: trace_recorder.as_mut(),
    };
    let summary =
        pipeline::run_processing_loop(raw_rx, &sanitizer, window, window_dur, &mut deps).await;
    tracing::info!(
        windows_closed = summary.windows_closed,
        events_processed = summary.events_processed,
        terminated_by = ?summary.terminated_by,
        "processing loop finished"
    );

    // Wait for collection task to finish
    let _ = collect_handle.await;

    tracing::info!("dipecsd stopped");
    Ok(())
}

fn profile_summary_interval_windows() -> u32 {
    std::env::var(PROFILE_SUMMARY_INTERVAL_ENV)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(10)
}

fn android_trace_jsonl_path(args: &[String]) -> Option<std::path::PathBuf> {
    let from_arg = args
        .windows(2)
        .find(|pair| pair[0] == "--android-trace-jsonl")
        .map(|pair| std::path::PathBuf::from(&pair[1]));
    from_arg.or_else(|| {
        std::env::var(ANDROID_TRACE_JSONL_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(std::path::PathBuf::from)
    })
}

fn runtime_trace_output_path(args: &[String]) -> Option<std::path::PathBuf> {
    let from_arg = args
        .windows(2)
        .find(|pair| pair[0] == "--trace-output")
        .map(|pair| std::path::PathBuf::from(&pair[1]));
    from_arg.or_else(|| {
        std::env::var(RUNTIME_TRACE_OUTPUT_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(std::path::PathBuf::from)
    })
}
