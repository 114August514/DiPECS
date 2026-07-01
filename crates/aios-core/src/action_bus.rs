//! 动作总线 — 事件派发与消费
//!
//! 基于 tokio mpsc channel 的多生产者-单消费者事件总线。
//! 用于 collector → core 的事件传输和 agent → core 的意图回传。
//!
//! `Sender` 端可以自由克隆分发给采集/推理任务；
//! `Receiver` 端由处理管道独占消费，不对外暴露。

use aios_spec::{IngestedRawEvent, IntentBatch};
use tokio::sync::mpsc;

/// 动作总线
///
/// 包含两条独立通道:
/// - raw_events: collector 向 core 推送已贴上 SourceTier 的原始事件
/// - intents: agent 向 core 回传意图批次
pub struct ActionBus {
    raw_events_rx: mpsc::Receiver<IngestedRawEvent>,
    raw_events_tx: mpsc::Sender<IngestedRawEvent>,
    intent_rx: mpsc::Receiver<IntentBatch>,
    intent_tx: mpsc::Sender<IntentBatch>,
}

impl ActionBus {
    pub fn new(capacity: usize) -> Self {
        let (raw_events_tx, raw_events_rx) = mpsc::channel(capacity);
        let (intent_tx, intent_rx) = mpsc::channel(capacity);
        Self {
            raw_events_rx,
            raw_events_tx,
            intent_rx,
            intent_tx,
        }
    }

    /// 获取原始事件发送端的克隆（给 collector 任务）
    pub fn raw_sender(&self) -> mpsc::Sender<IngestedRawEvent> {
        self.raw_events_tx.clone()
    }

    /// 获取意图发送端的克隆（给 agent 任务）
    pub fn intent_sender(&self) -> mpsc::Sender<IntentBatch> {
        self.intent_tx.clone()
    }

    /// 阻塞等待下一个原始事件（处理管道独占调用）
    pub async fn recv_raw(&mut self) -> Option<IngestedRawEvent> {
        self.raw_events_rx.recv().await
    }

    /// 阻塞等待下一个意图批次（策略审查独占调用）
    pub async fn recv_intent(&mut self) -> Option<IntentBatch> {
        self.intent_rx.recv().await
    }

    /// 拆出四个端点并消费掉 bus 自身。
    ///
    /// 与 `raw_sender()` + `recv_raw()` 的关键区别: `split` 交出 `raw_events_tx`
    /// 的所有权而不在结构体里残留一份。这样当采集侧的所有 sender 落地后, raw
    /// 通道才会真正关闭, `recv()` 返回 `None`——处理循环据此排空并 flush 最后一个
    /// 窗口再退出。若继续持有 bus, 内部那份 sender 会让通道永不关闭。
    #[allow(clippy::type_complexity)]
    pub fn split(
        self,
    ) -> (
        mpsc::Sender<IngestedRawEvent>,
        mpsc::Receiver<IngestedRawEvent>,
        mpsc::Sender<IntentBatch>,
        mpsc::Receiver<IntentBatch>,
    ) {
        (
            self.raw_events_tx,
            self.raw_events_rx,
            self.intent_tx,
            self.intent_rx,
        )
    }
}

impl Default for ActionBus {
    fn default() -> Self {
        Self::new(1024)
    }
}
