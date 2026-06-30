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
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

fn keepalive_batch() -> IntentBatch {
    IntentBatch {
        window_id: "w1".into(),
        intents: vec![Intent {
            intent_id: "intent-1".into(),
            intent_type: IntentType::Idle,
            confidence: 0.9,
            risk_level: RiskLevel::Low,
            suggested_actions: vec![SuggestedAction {
                action_type: ActionType::KeepAlive,
                target: Some("work:collector_heartbeat".into()),
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
fn forwarded_action_envelope_and_ok_maps_to_succeeded() {
    let (port, rx) = spawn_mock_bridge(
        br#"{"status":"ok","summary":"android_dispatched:KeepAlive","latency_us":11}"#,
    );
    let policy = PolicyEngine::default();
    let adapter = adapter_for(port);
    let lifecycle = ActionLifecycle::new(&policy, &adapter);
    let records = lifecycle.run(
        0,
        &keepalive_batch(),
        DecisionRoute::RuleBased,
        None,
        &permissive_capability(),
        &context_with_apps(&["com.example.app"]),
    );

    let payload = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("mock bridge should receive the forwarded envelope");
    let v: Value = serde_json::from_str(&payload).expect("payload is valid JSON");

    // 线信封形状(aios_spec::bridge::BridgeExecuteRequest)。
    assert_eq!(v["message_type"], "execute");
    let issued = v["issued_at_ms"].as_i64().expect("issued_at_ms present");
    let expires = v["expires_at_ms"].as_i64().expect("expires_at_ms present");
    assert!(expires > issued, "expires must be after issued");
    let action = v["action"]
        .as_str()
        .expect("action carried as the serialized AuthorizedAction string");
    assert!(
        action.contains("intent_id") && action.contains("KeepAlive"),
        "action must be the serialized AuthorizedAction",
    );
    // 认证标签 == 对 canonical(freshness window + length-prefixed action)的独立重算。
    let tag = v["auth"]["hmac_sha256"].as_str().expect("hmac present");
    assert_eq!(tag.len(), 64, "SHA-256 HMAC hex is 64 chars");
    assert_eq!(
        tag,
        recompute_envelope_hmac(TOKEN, issued, expires, action),
        "envelope HMAC must equal HMAC over the canonical execute input",
    );

    // 设备 {status:ok} → Succeeded,summary 透传设备上报值。
    assert_eq!(records.len(), 1);
    assert!(
        matches!(records[0].terminal, ActionState::Succeeded),
        "device ok must map to Succeeded, got {:?}",
        records[0].terminal,
    );
    assert_eq!(
        records[0]
            .outcome
            .as_ref()
            .expect("succeeded action has outcome")
            .summary,
        "android_dispatched:KeepAlive",
        "summary must carry the device-reported value",
    );
}

#[test]
fn device_rejection_maps_to_failed() {
    let (port, _rx) = spawn_mock_bridge(br#"{"status":"rejected","error":"bad hmac"}"#);
    let policy = PolicyEngine::default();
    let adapter = adapter_for(port);
    let lifecycle = ActionLifecycle::new(&policy, &adapter);
    let records = lifecycle.run(
        0,
        &keepalive_batch(),
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
