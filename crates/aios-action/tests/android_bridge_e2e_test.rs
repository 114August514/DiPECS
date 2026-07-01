//! AndroidAdapter 端到端转发测试。
//!
//! `AuthorizedAction` 的构造器是 `aios-core` 的 `pub(crate)`,外部 crate 无法直接
//! 伪造。本测试通过 `ActionLifecycle::run` 走完整 pipeline 驱动真实
//! `AndroidAdapter`(注入指向本地 mock `TcpListener` 的 `AndroidBridgeConfig`),
//! 接住转发的 `aios_spec::bridge` execute 信封,校验线协议并钉死设备回执的诚实映射。
//!
//! 与 `android_adapter.rs` 内联单测(直接调 `send_request` / `classify` / `compute_hmac`)
//! 互补:本测试覆盖「ActionLifecycle → AndroidAdapter → 线信封 → 回执 → 终态」全链路。
//!
//! 重点:
//! - 线信封 = `{message_type, issued_at_ms, expires_at_ms, auth:{hmac_sha256}, action}`;
//! - `auth.hmac_sha256` == 对 canonical(`dipecs.android.bridge.execute.v1` + freshness
//!   window + length-prefixed action 字节)的独立重算;
//! - 设备 `{status:"ok"}` → `Succeeded`(透传设备 summary);`{status:"rejected"}` → `Failed`。

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

use aios_action::{AndroidAdapter, AndroidBridgeConfig};
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::policy_engine::PolicyEngine;
use aios_spec::context::ContextSummary;
use aios_spec::governance::ActionState;
use aios_spec::intent::{
    ActionType, ActionUrgency, CapabilityLevel, DecisionRoute, Intent, IntentBatch, IntentType,
    RiskLevel, SuggestedAction,
};
use aios_spec::{SourceTier, StructuredContext};

const TOKEN: &str = "shared-secret";

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

fn context_with_apps(apps: &[&str]) -> StructuredContext {
    StructuredContext {
        window_id: "w1".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events: vec![],
        summary: ContextSummary {
            foreground_apps: apps.iter().map(|s| s.to_string()).collect(),
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    }
}

fn single_action_batch(action_type: ActionType, target: &str) -> IntentBatch {
    IntentBatch {
        window_id: "w1".into(),
        intents: vec![Intent {
            intent_id: "intent-1".into(),
            intent_type: IntentType::Idle,
            confidence: 0.9,
            risk_level: RiskLevel::Low,
            suggested_actions: vec![SuggestedAction {
                action_type,
                target: Some(target.into()),
                urgency: ActionUrgency::Immediate,
            }],
            rationale_tags: vec![],
        }],
        generated_at_ms: 1000,
        model: "test".into(),
    }
}

/// 起一个 mock socket,接一个连接、读到 EOF(adapter 半关写端后)拿到完整请求,经
/// channel 送回,再回送 `response`(如 `{status:"ok"}`)。
fn spawn_mock_bridge(response: &'static [u8]) -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock bridge");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = String::new();
            let _ = stream.read_to_string(&mut buf);
            let _ = tx.send(buf);
            let _ = stream.write_all(response);
            let _ = stream.flush();
            let _ = stream.shutdown(Shutdown::Write);
        }
    });
    (port, rx)
}

fn adapter_for(port: u16) -> AndroidAdapter {
    AndroidAdapter::new(AndroidBridgeConfig {
        host: "127.0.0.1".into(),
        port,
        auth_key: Some(TOKEN.into()),
    })
}

fn adapter_without_auth() -> AndroidAdapter {
    AndroidAdapter::new(AndroidBridgeConfig {
        host: "127.0.0.1".into(),
        port: 0,
        auth_key: None,
    })
}

/// 独立重算 envelope 的 `auth.hmac_sha256`:对 canonical 串(与 Rust/Kotlin 两侧一致)
/// 做 HMAC-SHA256。canonical 的 action 长度前缀用 UTF-8 字节数。
fn recompute_envelope_hmac(
    token: &str,
    issued_at_ms: i64,
    expires_at_ms: i64,
    action: &str,
) -> String {
    let canonical = format!(
        "dipecs.android.bridge.execute.v1\nissued_at_ms:{issued_at_ms}\nexpires_at_ms:{expires_at_ms}\naction:{}:{action}",
        action.len(),
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("hmac accepts any key");
    mac.update(canonical.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn forwarded_actions_envelope_and_ok_maps_to_succeeded() {
    // 遍历 AndroidAdapter::classify 下所有可转发的 action 类型,逐型钉死:execute 信封
    // 形状、canonical HMAC、内嵌 action_type==Rust Debug 串(防 serde rename 让 Serialize
    // 与 Debug 分叉)、设备 ok→Succeeded。NoOp 永不转发(本地 stub),故不在内。
    let cases = [
        (ActionType::KeepAlive, "work:collector_heartbeat"),
        (ActionType::ReleaseMemory, "cache:prefetch"),
        (ActionType::PrefetchFile, "url:https://example.test/a.json"),
        (ActionType::PreWarmProcess, "own:warmup"),
    ];

    for (action_type, target) in cases {
        let (port, rx) =
            spawn_mock_bridge(br#"{"status":"ok","summary":"android_executed","latency_us":7}"#);
        let policy = PolicyEngine::default();
        let adapter = adapter_for(port);
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let records = lifecycle.run(
            0,
            &single_action_batch(action_type.clone(), target),
            DecisionRoute::RuleBased,
            None,
            &permissive_capability(),
            &context_with_apps(&["com.example.app"]),
        );

        let payload = rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("mock bridge should receive envelope for {action_type:?}"));
        let v: Value = serde_json::from_str(&payload).expect("payload is valid JSON");

        // 线信封形状(aios_spec::bridge::BridgeExecuteRequest)。
        assert_eq!(v["message_type"], "execute", "{action_type:?}");
        let issued = v["issued_at_ms"].as_i64().expect("issued_at_ms present");
        let expires = v["expires_at_ms"].as_i64().expect("expires_at_ms present");
        assert!(
            expires > issued,
            "{action_type:?}: expires must be after issued"
        );
        // freshness 窗口必须恰为 ANDROID_ACTION_PAYLOAD_TTL_MS(60_000ms)。`expires > issued`
        // 挡不住 TTL 常量被改大/改小或计算漂移;此处钉死窗口宽度,守住客户端 freshness 构造。
        // 常量在 aios-action 内为 crate-private,故按本仓库 golden 值惯例直接钉字面量。
        assert_eq!(
            expires - issued,
            60_000,
            "{action_type:?}: freshness window must equal ANDROID_ACTION_PAYLOAD_TTL_MS (60_000ms)",
        );
        let action = v["action"]
            .as_str()
            .expect("action carried as the serialized AuthorizedAction string");
        // 认证标签 == 对 canonical(freshness window + length-prefixed action)的独立重算。
        let tag = v["auth"]["hmac_sha256"].as_str().expect("hmac present");
        assert_eq!(
            tag.len(),
            64,
            "{action_type:?}: SHA-256 HMAC hex is 64 chars"
        );
        assert_eq!(
            tag,
            recompute_envelope_hmac(TOKEN, issued, expires, action),
            "{action_type:?}: envelope HMAC must equal HMAC over canonical execute input",
        );
        // 内嵌 action_type 字符串 == Rust Debug 串。
        let action_value: Value =
            serde_json::from_str(action).expect("action string is itself valid JSON");
        assert_eq!(
            action_value["action"]["action_type"]
                .as_str()
                .expect("action_type is a string"),
            format!("{action_type:?}"),
            "embedded action_type must match Debug string for {action_type:?}",
        );

        // 设备 {status:ok} → Succeeded,summary 透传设备上报值。
        assert_eq!(records.len(), 1, "{action_type:?}: one audit record");
        assert!(
            matches!(records[0].terminal, ActionState::Succeeded),
            "{action_type:?}: device ok must map to Succeeded, got {:?}",
            records[0].terminal,
        );
        assert_eq!(
            records[0]
                .outcome
                .as_ref()
                .expect("succeeded action has outcome")
                .summary,
            "android_executed",
            "{action_type:?}: summary must carry the device-reported value",
        );
    }
}

#[test]
fn device_rejection_maps_to_failed() {
    let (port, _rx) = spawn_mock_bridge(br#"{"status":"rejected","error":"bad hmac"}"#);
    let policy = PolicyEngine::default();
    let adapter = adapter_for(port);
    let lifecycle = ActionLifecycle::new(&policy, &adapter);
    let records = lifecycle.run(
        0,
        &single_action_batch(ActionType::KeepAlive, "work:collector_heartbeat"),
        DecisionRoute::RuleBased,
        None,
        &permissive_capability(),
        &context_with_apps(&["com.example.app"]),
    );

    assert_eq!(records.len(), 1);
    assert!(
        matches!(records[0].terminal, ActionState::Failed),
        "device rejection must map to Failed, got {:?}",
        records[0].terminal,
    );
    assert!(
        records[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("rejected"),
        "error should explain the rejection: {:?}",
        records[0].error,
    );
}

/// 启动一个会校验 HMAC 和 freshness 的 mock bridge。
/// - 若 HMAC 不匹配,返回 rejected("bad hmac")。
/// - 若当前时间已超过 expires_at_ms,返回 rejected("expired")。
/// - 若同一 envelope 已被接受过(replay),返回 rejected("replayed")。
fn spawn_verifying_mock_bridge() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock bridge");
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_clone = seen.clone();

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
            }
            let text = String::from_utf8_lossy(&buf).to_string();
            let response = verify_request(&text, &mut seen_clone.lock().unwrap());
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(Shutdown::Write);
        }
    });

    (port, seen)
}

fn verify_request(text: &str, seen: &mut Vec<String>) -> String {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return r#"{"status":"rejected","error":"invalid json"}"#.to_string(),
    };

    let issued = value["issued_at_ms"].as_i64().unwrap_or(0);
    let expires = value["expires_at_ms"].as_i64().unwrap_or(0);
    let action = value["action"].as_str().unwrap_or("");
    let tag = value["auth"]["hmac_sha256"].as_str().unwrap_or("");

    let expected = recompute_envelope_hmac(TOKEN, issued, expires, action);
    if tag != expected {
        return r#"{"status":"rejected","error":"bad hmac"}"#.to_string();
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    if now > expires {
        return r#"{"status":"rejected","error":"expired"}"#.to_string();
    }

    if seen.iter().any(|payload| payload == text) {
        return r#"{"status":"rejected","error":"replayed"}"#.to_string();
    }
    seen.push(text.to_string());

    r#"{"status":"ok","summary":"android_executed","latency_us":7}"#.to_string()
}

#[test]
fn replayed_envelope_is_rejected() {
    let (port, seen) = spawn_verifying_mock_bridge();

    // 第一次正常执行,应成功。
    let policy = PolicyEngine::default();
    let adapter = adapter_for(port);
    let lifecycle = ActionLifecycle::new(&policy, &adapter);
    let records = lifecycle.run(
        0,
        &single_action_batch(ActionType::KeepAlive, "work:collector_heartbeat"),
        DecisionRoute::RuleBased,
        None,
        &permissive_capability(),
        &context_with_apps(&["com.example.app"]),
    );
    assert!(matches!(records[0].terminal, ActionState::Succeeded));

    // 把同一条 TCP payload 原样再发一次(模拟重放攻击),server 应拒绝。
    let replay_payload = seen
        .lock()
        .unwrap()
        .first()
        .cloned()
        .expect("server saw one payload");

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(replay_payload.as_bytes()).unwrap();
    stream.flush().unwrap();
    stream.shutdown(Shutdown::Write).unwrap();

    let mut response = String::new();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let _ = stream.read_to_string(&mut response);
    assert!(
        response.contains("replayed"),
        "replayed envelope should be rejected, got: {response}"
    );
}

#[test]
fn expired_envelope_is_rejected() {
    let (port, _seen) = spawn_verifying_mock_bridge();

    // 构造一个已过期但 HMAC 合法的 envelope 直接发过去。
    let issued_at_ms = 0i64;
    let expires_at_ms = 1i64;
    let action = serde_json::json!({
        "intent_id": "i1",
        "coord": {"window_ordinal":0,"intent_ordinal":0,"action_ordinal":0},
        "action": {"action_type":"KeepAlive","target":"work:collector_heartbeat","urgency":"Immediate"},
        "effect": "PureRead",
        "authorized_at_ms": 0,
        "risk_level": "Low",
        "authorized_by": "test",
    })
    .to_string();
    let hmac = recompute_envelope_hmac(TOKEN, issued_at_ms, expires_at_ms, &action);

    let payload = serde_json::json!({
        "message_type": "execute",
        "issued_at_ms": issued_at_ms,
        "expires_at_ms": expires_at_ms,
        "auth": {"hmac_sha256": hmac},
        "action": action,
    })
    .to_string();

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(payload.as_bytes()).unwrap();
    stream.flush().unwrap();
    stream.shutdown(Shutdown::Write).unwrap();

    let mut response = String::new();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let _ = stream.read_to_string(&mut response);
    assert!(
        response.contains("expired"),
        "expired envelope should be rejected, got: {response}"
    );
}

#[test]
fn missing_auth_key_fails_before_connecting() {
    // AndroidAdapter 构造时注入 auth_key;如果缺失,forward 应在发任何网络请求前失败,
    // 并给出配置错误而非连接错误。
    let policy = PolicyEngine::default();
    let adapter = adapter_without_auth();
    let lifecycle = ActionLifecycle::new(&policy, &adapter);
    let records = lifecycle.run(
        0,
        &single_action_batch(ActionType::KeepAlive, "work:collector_heartbeat"),
        DecisionRoute::RuleBased,
        None,
        &permissive_capability(),
        &context_with_apps(&["com.example.app"]),
    );

    assert_eq!(records.len(), 1);
    assert!(
        matches!(records[0].terminal, ActionState::Failed),
        "missing auth_key must map to Failed, got {:?}",
        records[0].terminal,
    );
    let error = records[0].error.as_deref().unwrap_or_default();
    assert!(
        error.contains("TOKEN is required"),
        "error should mention required token: {error}"
    );
}
