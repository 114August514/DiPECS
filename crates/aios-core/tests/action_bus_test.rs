//! 验证 ActionBus 的事件通道

use aios_core::action_bus::ActionBus;
use aios_spec::*;

fn make_raw_event() -> IngestedRawEvent {
    IngestedRawEvent {
        raw_event: RawEvent::ProcStateChange(ProcStateEvent {
            timestamp_ms: 1000,
            pid: 42,
            uid: 10123,
            package_name: Some("com.test.app".into()),
            vm_rss_kb: 128000,
            vm_swap_kb: 0,
            threads: 8,
            oom_score: 0,
            io_read_mb: 1,
            io_write_mb: 2,
            state: ProcState::Running,
        }),
        source_tier: SourceTier::Daemon,
    }
}

fn make_intent_batch() -> IntentBatch {
    IntentBatch {
        window_id: "w1".into(),
        intents: vec![Intent {
            intent_id: "int-1".into(),
            intent_type: IntentType::Idle,
            confidence: 0.5,
            risk_level: RiskLevel::Low,
            suggested_actions: vec![SuggestedAction {
                action_type: ActionType::NoOp,
                target: None,
                urgency: ActionUrgency::IdleTime,
            }],
            rationale_tags: vec![],
        }],
        generated_at_ms: 5000,
        model: "test".into(),
    }
}

#[tokio::test]
async fn test_send_raw_event_received() {
    let mut bus = ActionBus::new(4);
    bus.raw_sender().send(make_raw_event()).await.unwrap();

    let received = bus.recv_raw().await;
    assert!(received.is_some());
}

#[tokio::test]
async fn test_send_intent_received() {
    let mut bus = ActionBus::new(4);
    bus.intent_sender().send(make_intent_batch()).await.unwrap();

    let batch = bus.recv_intent().await.unwrap();
    assert_eq!(batch.model, "test");
    assert_eq!(batch.intents.len(), 1);
}

#[tokio::test]
async fn test_sender_closed_after_bus_dropped() {
    let bus = ActionBus::new(4);
    let raw_tx = bus.raw_sender();
    let intent_tx = bus.intent_sender();
    drop(bus); // drops receivers → channel closes

    let result = raw_tx.send(make_raw_event()).await;
    assert!(result.is_err(), "send should fail after rx dropped");

    let result = intent_tx.send(make_intent_batch()).await;
    assert!(result.is_err());
}

#[test]
fn test_action_bus_default_creates() {
    let bus = ActionBus::default();
    assert!(!bus.raw_sender().is_closed());
    assert!(!bus.intent_sender().is_closed());
}

#[tokio::test]
async fn test_split_raw_channel_closes_when_sender_dropped() {
    // split 交出 sender 所有权而不残留内部副本: 唯一的 raw sender 落地后,
    // recv 必须返回 None。daemon 处理循环靠这个 None 来 flush 最后窗口并退出。
    let (raw_tx, mut raw_rx, _intent_tx, _intent_rx) = ActionBus::new(4).split();

    raw_tx.send(make_raw_event()).await.unwrap();
    drop(raw_tx);

    // 先收到已缓冲的事件, 再收到通道关闭信号。
    assert!(
        raw_rx.recv().await.is_some(),
        "buffered event delivered first"
    );
    assert!(
        raw_rx.recv().await.is_none(),
        "raw channel must close once the only sender is dropped"
    );
}

#[tokio::test]
async fn test_split_intent_channel_roundtrips() {
    let (_raw_tx, _raw_rx, intent_tx, mut intent_rx) = ActionBus::new(4).split();
    intent_tx.send(make_intent_batch()).await.unwrap();

    let batch = intent_rx.recv().await.unwrap();
    assert_eq!(batch.model, "test");
    assert_eq!(batch.intents.len(), 1);
}
