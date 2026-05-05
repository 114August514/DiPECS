//! # dipecsd — DiPECS 系统守护进程
//!
//! 部署路径: `/system/bin/dipecsd`
//! 启动方式: `dipecsd [--no-daemon] [--verbose]`
//!
//! ## 运行模式
//!
//! - **daemon 模式** (默认): fork 到后台, 持续采集系统事件
//! - **--no-daemon**: 前台运行, 用于调试和开发
//!
//! ## 数据流 (v0.2 — 2-task 管道)
//!
//! ```text
//! Task 1 (采集):  BinderProbe + ProcReader + SysCollector → bus.raw_events_tx
//! Task 2 (处理):  bus.raw_events_rx → PrivacyAirGap → WindowAggregator
//!                    → MockCloudProxy → PolicyEngine → ActionExecutor
//! ```
//!
//! ## 当前实现状态 (2026-05-05)
//!
//! - ProcReader: 可用 (Linux/Android 均可)
//! - SystemStateCollector: 可用 (Linux/Android 均可, 电池/网络 fallback)
//! - BinderProbe: 接口完成, eBPF attach 待真机验证
//! - Cloud LLM 通信: MockCloudProxy (模拟返回)
//! - Action 执行: 骨架 (tracing 记录)

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aios_adapter::{
    binder_probe::BinderProbe,
    collection_stats::RawEventStats,
    daemon,
    proc_reader::{self, ProcReader},
    system_collector::SystemStateCollector,
};
use aios_core::action_bus::ActionBus;
use aios_core::context_builder::WindowAggregator;
use aios_core::policy_engine::PolicyEngine;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_kernel::DefaultActionExecutor;
use aios_spec::traits::ActionExecutor;
use aios_spec::traits::PrivacySanitizer;
use aios_spec::RawEvent;

use aios_agent::MockCloudProxy;

/// 系统状态采集间隔 (秒)
const SYS_POLL_INTERVAL_SECS: u64 = 30;
/// Binder 事件轮询间隔 (毫秒)
const BINDER_POLL_INTERVAL_MS: u64 = 100;
/// 上下文窗口时长 (秒)
const WINDOW_DURATION_SECS: u64 = 10;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("dipecs=info".parse()?),
        )
        .init();

    // 2. 解析命令行参数
    let args: Vec<String> = std::env::args().collect();
    let no_daemon = args.iter().any(|a| a == "--no-daemon");

    if !no_daemon {
        daemon::daemonize();
    }
    tracing::info!("dipecsd starting (no-daemon={})", no_daemon);

    // 3. 初始化 ActionBus 和关闭信号
    let mut bus = ActionBus::new(4096);
    let mut shutdown_rx = daemon::install_signal_handlers();

    // ---- Task 1: 采集 ----
    let collect_raw_tx = bus.raw_events_tx.clone();
    let mut collect_shutdown = shutdown_rx.resubscribe();
    let collect_handle = tokio::spawn(async move {
        tracing::info!("collection task started");

        let mut binder_probe = BinderProbe::new();
        match binder_probe.try_init() {
            Ok(true) => tracing::info!("Binder probe initialized with eBPF"),
            Ok(false) => {
                tracing::warn!("Binder probe unavailable — running without IPC monitoring")
            },
            Err(e) => tracing::error!("Binder probe init failed: {}", e),
        }

        let mut prev_proc_snapshots: HashMap<u32, proc_reader::ProcSnapshot> = HashMap::new();
        let mut last_sys_poll = SystemTime::now() - Duration::from_secs(SYS_POLL_INTERVAL_SECS);

        loop {
            let now = timestamp_ms();

            // /proc 轮询
            {
                let snapshots = ProcReader::scan_all();
                let curr_map: HashMap<u32, proc_reader::ProcSnapshot> =
                    snapshots.iter().map(|s| (s.pid, s.clone())).collect();

                let changed = proc_reader::diff_snapshots(&prev_proc_snapshots, &curr_map);
                for snap in &changed {
                    let event = RawEvent::ProcStateChange(ProcReader::to_event(snap, now));
                    if collect_raw_tx.send(event).await.is_err() {
                        tracing::debug!("collection: raw channel closed");
                        return; // processing task exited
                    }
                }
                prev_proc_snapshots = curr_map;
            }

            // 系统状态采集
            {
                let elapsed = SystemTime::now()
                    .duration_since(last_sys_poll)
                    .unwrap_or_default();
                if elapsed.as_secs() >= SYS_POLL_INTERVAL_SECS {
                    let sys_event = SystemStateCollector::snapshot(now);
                    let event = RawEvent::SystemState(sys_event);
                    if collect_raw_tx.send(event).await.is_err() {
                        tracing::debug!("collection: raw channel closed");
                        return;
                    }
                    last_sys_poll = SystemTime::now();
                }
            }

            // Binder 事件轮询
            {
                let binder_events = binder_probe.poll();
                for tx in &binder_events {
                    let event = RawEvent::BinderTransaction(tx.to_event());
                    if collect_raw_tx.send(event).await.is_err() {
                        tracing::debug!("collection: raw channel closed");
                        return;
                    }
                }
            }

            // 检查退出信号 (non-blocking)
            if collect_shutdown.try_recv().is_ok() {
                tracing::info!("collection task shutting down");
                return;
            }

            tokio::time::sleep(Duration::from_millis(BINDER_POLL_INTERVAL_MS)).await;
        }
    });

    // ---- Task 2: 处理管道 (运行在主 task 上) ----
    tracing::info!("processing task started");

    let sanitizer = DefaultPrivacyAirGap;
    let policy = PolicyEngine::default();
    let executor = DefaultActionExecutor;
    let window_dur = Duration::from_secs(WINDOW_DURATION_SECS);
    let mut window = WindowAggregator::new(WINDOW_DURATION_SECS, timestamp_ms());
    let mut raw_stats = RawEventStats::default();
    let mut window_deadline = Instant::now() + window_dur;

    loop {
        let remaining = if window_deadline > Instant::now() {
            window_deadline - Instant::now()
        } else {
            Duration::ZERO
        };

        let raw_event = tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::info!("processing task shutting down");
                break;
            }
            event = bus.raw_events_rx.recv() => {
                event
            }
            _ = tokio::time::sleep(remaining) => {
                None // timeout → window close
            }
        };

        match raw_event {
            Some(raw) => {
                raw_stats.record(&raw);
                let sanitized = sanitizer.sanitize(raw);
                window.push(sanitized);
            },
            None => {
                // Channel closed (collection task exited) — flush and exit
                tracing::info!("raw event channel closed, flushing remaining events");
                let window_stats = std::mem::take(&mut raw_stats);
                if let Some(ctx) = window.close(timestamp_ms()) {
                    process_window(&ctx, &policy, &executor, &window_stats);
                }
                break;
            },
        }

        // Check if window should close
        if Instant::now() >= window_deadline {
            let window_stats = std::mem::take(&mut raw_stats);
            if let Some(ctx) = window.close(timestamp_ms()) {
                process_window(&ctx, &policy, &executor, &window_stats);
            }
            window_deadline = Instant::now() + window_dur;
        }
    }

    // Wait for collection task to finish
    let _ = collect_handle.await;

    tracing::info!("dipecsd stopped");
    Ok(())
}

/// 处理一个上下文窗口: mock agent → validate → execute
fn process_window(
    ctx: &aios_spec::StructuredContext,
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

    let batch = MockCloudProxy::evaluate(ctx);
    let decisions = policy.evaluate_batch(&batch);

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
    }

    tracing::info!(
        window_id = %ctx.window_id,
        intents_total = decisions.len(),
        actions_executed = executed,
        "window processed"
    );
}

fn timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
