//! `AndroidAdapter` 端到端集成测试。
//!
//! `AuthorizedAction` 只能由 `aios-core` 的 `pub(crate) seal` 经 `ActionLifecycle`
//! 铸造，外部 crate 无法伪造；因此这里通过完整 pipeline 驱动 `AndroidAdapter`，
//! 用一个 mock localhost TCP responder 扮演设备侧 bridge，断言审计终态。
//!
//! 重点验证"诚实"语义：设备未回执 / 连接被拒 / 设备拒绝 → 终态 `Failed`，
//! 而不再把一次成功的 TCP 写谎报为 `Succeeded`。

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use aios_action::{AndroidAdapter, AndroidBridgeConfig};
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::policy_engine::PolicyEngine;
use aios_spec::context::ContextSummary;
use aios_spec::governance::ActionState;
use aios_spec::intent::{
    ActionType, ActionUrgency, CapabilityLevel, Intent, IntentBatch, IntentType, RiskLevel,
    SuggestedAction,
};
use aios_spec::{SourceTier, StructuredContext};

fn single_action_batch(action: SuggestedAction) -> IntentBatch {
    IntentBatch {
        window_id: "w1".into(),
        intents: vec![Intent {
            intent_id: "intent-1".into(),
            intent_type: IntentType::Idle,
            confidence: 0.9,
            risk_level: RiskLevel::Low,
            suggested_actions: vec![action],
            rationale_tags: vec![],
        }],
        generated_at_ms: 1000,
        model: "test".into(),
    }
}

/// Build a context in which `targets` are known operable entities. The policy
/// engine's anti-hallucination gate denies any action whose target never
/// appeared in the window, so a forwardable action must register its target here.
fn context_with_targets(targets: &[&str]) -> StructuredContext {
    StructuredContext {
        window_id: "w1".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events: vec![],
        summary: ContextSummary {
            foreground_apps: targets.iter().map(|t| t.to_string()).collect(),
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    }
}

fn permissive_capability() -> CapabilityLevel {
    CapabilityLevel {
        max_risk: RiskLevel::High,
        allowed_actions: vec![
            ActionType::NoOp,
            ActionType::PreWarmProcess,
            ActionType::PrefetchFile,
            ActionType::KeepAlive,
            ActionType::ReleaseMemory,
        ],
    }
}

fn prefetch_url() -> SuggestedAction {
    SuggestedAction {
        action_type: ActionType::PrefetchFile,
        target: Some("url:https://example.test/feed.json".into()),
        urgency: ActionUrgency::Immediate,
    }
}

fn config_for(port: u16) -> AndroidBridgeConfig {
    AndroidBridgeConfig {
        host: "127.0.0.1".into(),
        port,
        auth_key: Some("shared-secret".into()),
    }
}

fn run_single(
    adapter: &AndroidAdapter,
    action: SuggestedAction,
) -> aios_spec::governance::AuditRecord {
    // Register the action's target (if any) as a known context entity so the
    // policy gate approves it and the action actually reaches the adapter.
    let ctx = match action.target.as_deref() {
        Some(target) => context_with_targets(&[target]),
        None => context_with_targets(&[]),
    };
    let policy = PolicyEngine::default();
    let lifecycle = ActionLifecycle::new(&policy, adapter);
    let mut records = lifecycle.run(
        0,
        &single_action_batch(action),
        &permissive_capability(),
        &ctx,
    );
    assert_eq!(records.len(), 1);
    records.remove(0)
}

/// 启动一次性 responder：读到 EOF 后回送 `response`，并把收到的请求经 channel 送回。
fn spawn_responder(response: &'static [u8]) -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&buf).to_string());
            let _ = stream.write_all(response);
            let _ = stream.flush();
            let _ = stream.shutdown(Shutdown::Write);
        }
    });
    (port, rx)
}

/// 取一个空闲端口（绑定后立即释放），用于"无服务端"的失败用例。
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

#[test]
fn forwards_and_succeeds_with_device_outcome() {
    let (port, rx) =
        spawn_responder(br#"{"status":"ok","summary":"device_prefetched","latency_us":4242}"#);
    let adapter = AndroidAdapter::new(config_for(port));

    let record = run_single(&adapter, prefetch_url());

    assert!(
        matches!(record.terminal, ActionState::Succeeded),
        "expected Succeeded, got {:?}",
        record.terminal
    );
    let outcome = record
        .outcome
        .as_ref()
        .expect("succeeded action has outcome");
    assert_eq!(
        outcome.summary, "device_prefetched",
        "summary must come from the device, not a synthetic constant"
    );

    // The envelope must carry the execute message + an HMAC tag bound to the action.
    let request = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("responder saw a request");
    let value: serde_json::Value = serde_json::from_str(&request).expect("request is JSON");
    assert_eq!(value["message_type"], "execute");
    let tag = value["auth"]["hmac_sha256"].as_str().expect("hmac present");
    assert_eq!(tag.len(), 64, "SHA-256 HMAC hex is 64 chars");
    let action_str = value["action"].as_str().expect("action carried as string");
    assert!(
        action_str.contains("intent_id"),
        "action must be the serialized AuthorizedAction"
    );
}

#[test]
fn marks_failed_when_device_rejects() {
    let (port, _rx) = spawn_responder(br#"{"status":"rejected","error":"token refused"}"#);
    let adapter = AndroidAdapter::new(config_for(port));

    let record = run_single(&adapter, prefetch_url());

    assert!(
        matches!(record.terminal, ActionState::Failed),
        "device rejection must be Failed, got {:?}",
        record.terminal
    );
    assert!(
        record
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("rejected"),
        "error should explain the rejection: {:?}",
        record.error
    );
}

/// The headline regression: fire-and-forget used to report Succeeded here.
#[test]
fn fails_closed_when_no_responder() {
    let adapter = AndroidAdapter::new(config_for(free_port()));

    let record = run_single(&adapter, prefetch_url());

    assert!(
        matches!(record.terminal, ActionState::Failed),
        "an unreachable bridge must be Failed, not Succeeded; got {:?}",
        record.terminal
    );
    assert!(
        record
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("connect Android bridge"),
        "error should name the failed connect: {:?}",
        record.error
    );
}

#[test]
fn non_forwardable_action_uses_local_stub_without_touching_bridge() {
    // Point at a dead port: if NoOp tried to forward it would fail. It must not.
    let adapter = AndroidAdapter::new(config_for(free_port()));

    let record = run_single(
        &adapter,
        SuggestedAction {
            action_type: ActionType::NoOp,
            target: None,
            urgency: ActionUrgency::Immediate,
        },
    );

    assert!(
        matches!(record.terminal, ActionState::Succeeded),
        "NoOp falls back to the local stub and succeeds, got {:?}",
        record.terminal
    );
    assert_eq!(record.outcome.as_ref().unwrap().summary, "noop");
}
