//! Task-1 采集循环的可测抽象。
//!
//! 把「事件从哪来」抽象成 [`RawEventSource`],与「怎么周期性推进通道 + 响应
//! shutdown」的循环骨架 [`run_collection_loop`] 解耦。
//!
//! - 生产用 [`SystemRawEventSource`]:封装 proc/sys/binder/android 四类真实采集器
//!   及各自的轮询节奏。
//! - 测试注入合成源,用**真实的采集循环 + 真实通道**验证双任务链路,只把「事件
//!   从哪来」换成替身(真实采集器要读真机 /proc、电量、Binder,不可确定性单测)。

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use aios_collector::{
    android_jsonl::AndroidJsonlTailer,
    binder_probe::BinderProbe,
    proc_reader::{self, ProcReader},
    system_collector::SystemStateCollector,
};
use aios_core::collector_ingress::RustCollectorIngress;
use aios_spec::{IngestedRawEvent, RawEvent};
use tokio::sync::{broadcast, mpsc};

use crate::pipeline::timestamp_ms;

/// System state collection interval, in seconds.
const SYS_POLL_INTERVAL_SECS: u64 = 30;
/// Binder event polling interval, in milliseconds. Also the base loop tick.
pub(crate) const BINDER_POLL_INTERVAL_MS: u64 = 100;
/// Android JSONL file polling interval, in milliseconds.
const ANDROID_JSONL_POLL_INTERVAL_MS: u64 = 500;

/// 采集事件源:每个 tick 被 poll 一次,返回本 tick 新产生、已贴 `SourceTier` 的事件。
///
/// 各源自身的轮询节奏(如 sys 每 30s、android 每 500ms)由实现内部维护,
/// 循环骨架只负责「拿到就推进通道」。
pub(crate) trait RawEventSource: Send {
    fn poll(&mut self, now_ms: i64) -> Vec<IngestedRawEvent>;
}

/// Task-1 采集循环:周期性 poll 事件源,把事件推进 raw 通道;收到 shutdown 即返回。
///
/// 返回后 `raw_tx` 落地 → 通道关闭 → Task-2 (`run_processing_loop`) 的 `recv()` 返回
/// `None` → flush 最后一个窗口收尾。这是 daemon 优雅停机的真实路径。
pub(crate) async fn run_collection_loop(
    mut source: Box<dyn RawEventSource>,
    raw_tx: mpsc::Sender<IngestedRawEvent>,
    mut shutdown_rx: broadcast::Receiver<()>,
    poll_interval: Duration,
) {
    tracing::info!("collection task started");
    loop {
        // shutdown 只在两次 poll 之间被检查; source.poll() 是同步 I/O, 必须保持有界——
        // 一次长阻塞会同时拖住采集与「靠通道关闭收尾」的处理任务的优雅停机。若某采集器
        // 可能长阻塞, 应改走 spawn_blocking, 而不是在处理循环加 shutdown 分支(那会重新
        // 引入丢最后一个未满窗口的 bug)。
        let now = timestamp_ms();
        for event in source.poll(now) {
            if raw_tx.send(event).await.is_err() {
                tracing::debug!("collection: raw channel closed");
                return;
            }
        }
        // 每 tick 结束检查一次 shutdown(非阻塞);先 poll 后检查,保证收到停机信号
        // 前当前 tick 已产生的事件都已推进通道。只有 Empty(确无信号)才继续;收到信号
        // (Ok)、sender 全 drop(Closed)、漏读(Lagged)都收尾——否则信号任务异常退出后
        // 本循环会永远空转(release 下 panic=abort 兜底, 但 debug 下会真挂)。
        match shutdown_rx.try_recv() {
            Err(broadcast::error::TryRecvError::Empty) => {},
            _ => {
                tracing::info!("collection task shutting down");
                return;
            },
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// 生产事件源:封装真实的 proc/sys/binder/android 采集器及各自的轮询节奏。
pub(crate) struct SystemRawEventSource {
    ingress: RustCollectorIngress,
    android_tailer: Option<AndroidJsonlTailer>,
    binder_probe: BinderProbe,
    prev_proc_snapshots: HashMap<u32, proc_reader::ProcSnapshot>,
    // 轮询节奏用单调钟 (Instant) 度量,不受墙钟 NTP 跳变影响 (墙钟向后跳会让
    // duration_since 持续 Err→ZERO,采样一直被跳过直到墙钟追回)。None = 尚未采过,立即触发。
    last_sys_poll: Option<Instant>,
    last_android_jsonl_poll: Option<Instant>,
}

impl SystemRawEventSource {
    pub(crate) fn new(android_trace_jsonl: Option<PathBuf>) -> Self {
        let mut binder_probe = BinderProbe::new();
        match binder_probe.try_init() {
            Ok(true) => tracing::info!("Binder probe initialized with eBPF"),
            Ok(false) => {
                tracing::warn!("Binder probe unavailable; running without IPC monitoring")
            },
            Err(e) => tracing::error!("Binder probe init failed: {}", e),
        }
        Self {
            ingress: RustCollectorIngress,
            android_tailer: android_trace_jsonl.map(AndroidJsonlTailer::new),
            binder_probe,
            prev_proc_snapshots: HashMap::new(),
            last_sys_poll: None,
            last_android_jsonl_poll: None,
        }
    }
}

impl RawEventSource for SystemRawEventSource {
    fn poll(&mut self, now_ms: i64) -> Vec<IngestedRawEvent> {
        let mut events = Vec::new();

        // /proc polling (每 tick)。scan_all() 的 Vec 之后不再用,直接 into_iter 移动进
        // map,省掉每 tick 上百次 ProcSnapshot 全量 clone(10Hz×数百进程的分配器抖动)。
        let snapshots = ProcReader::scan_all();
        let curr_map: HashMap<u32, proc_reader::ProcSnapshot> =
            snapshots.into_iter().map(|s| (s.pid, s)).collect();
        let changed = proc_reader::diff_snapshots(&self.prev_proc_snapshots, &curr_map);
        for snap in &changed {
            events.push(self.ingress.accept_internal(
                RawEvent::ProcStateChange(ProcReader::to_event(snap, now_ms)),
                "ProcReader",
                now_ms,
            ));
        }
        self.prev_proc_snapshots = curr_map;

        // System state collection (节奏: 每 SYS_POLL_INTERVAL_SECS)
        if self
            .last_sys_poll
            .is_none_or(|t| t.elapsed().as_secs() >= SYS_POLL_INTERVAL_SECS)
        {
            let sys_event = SystemStateCollector::snapshot(now_ms);
            events.push(self.ingress.accept_internal(
                RawEvent::SystemState(sys_event),
                "SystemStateCollector",
                now_ms,
            ));
            self.last_sys_poll = Some(Instant::now());
        }

        // Binder event polling (每 tick)
        for tx in &self.binder_probe.poll() {
            events.push(self.ingress.accept_internal(
                RawEvent::BinderTransaction(tx.to_event()),
                "BinderProbe",
                now_ms,
            ));
        }

        // Android collector JSONL ingress (节奏: 每 ANDROID_JSONL_POLL_INTERVAL_MS)。
        // 这是 phase-1 app 的生产桥:app 负责公开 Android API 采集,Rust 负责
        // schema 校验与下游隐私管线。
        if let Some(tailer) = self.android_tailer.as_mut() {
            if self.last_android_jsonl_poll.is_none_or(|t| {
                t.elapsed() >= Duration::from_millis(ANDROID_JSONL_POLL_INTERVAL_MS)
            }) {
                match tailer.poll() {
                    Ok(envelopes) => {
                        for envelope in envelopes {
                            match self.ingress.accept(envelope) {
                                Ok(event) => events.push(event),
                                Err(error) => {
                                    tracing::warn!(error = %error, "Android JSONL envelope rejected")
                                },
                            }
                        }
                    },
                    Err(error) => {
                        tracing::warn!(
                            path = %tailer.path().display(),
                            error = %error,
                            "Android JSONL poll failed"
                        );
                    },
                }
                self.last_android_jsonl_poll = Some(Instant::now());
            }
        }

        events
    }
}
