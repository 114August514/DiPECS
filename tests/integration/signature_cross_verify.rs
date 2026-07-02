//! HMAC 签名交叉验证 baseline。
//!
//! `aios-cli::android_bridge` 用手写 ipad/opad 实现了 HMAC-SHA256；本模块用标准
//! `hmac` crate 独立重算，交叉验证**被测代码**的签名实现。
//!
//! 核心验证 `action_signature_matches_production` 走真实生产路径:调
//! `send_action` 让它把带 `action_signature` 的完整 payload 写到 mock socket,
//! 捕获后用标准 crate 对相同 canonical 格式独立重算,断言两者相等——这同时钉死
//! 生产侧的 canonical 格式与手写 HMAC 的正确性。
//!
//! 辅以 RFC 4231 已知向量(锚定标准 crate 与手写实现等价)与敏感性测试。

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::sync::mpsc;
use std::thread;

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

use aios_cli::android_bridge::send_action;

/// 用标准 `hmac` crate 独立重算 HMAC-SHA256，输出小写十六进制。
fn recompute_hmac(key: &[u8], message: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 独立重建 `android_bridge::action_signature` 的 canonical 消息，然后用
/// `recompute_hmac` 签名。格式必须与生产侧精确一致：
///
/// ```text
/// dipecs.android.action.v1\n
/// issued_at_ms:{issued_at_ms}\n
/// expires_at_ms:{expires_at_ms}\n
/// action_type:{len}:{action_type}\n
/// target:{len}:{target}\n
/// urgency:{len}:{urgency}
/// ```
fn recompute_action_signature(
    token: &str,
    issued_at_ms: i64,
    expires_at_ms: i64,
    action_type: &str,
    target: &str,
    urgency: &str,
) -> String {
    let canonical = format!(
        "dipecs.android.action.v1\nissued_at_ms:{issued_at_ms}\nexpires_at_ms:{expires_at_ms}\naction_type:{}:{action_type}\ntarget:{}:{target}\nurgency:{}:{urgency}",
        action_type.len(),
        target.len(),
        urgency.len(),
    );
    recompute_hmac(token.as_bytes(), canonical.as_bytes())
}

/// mock bridge:接受一个连接、读完请求(经 channel 送回)、回送 `response`
/// 后半关写端。与 `action_success_rate.rs` 同型,此处保持独立副本。
/// `send_action` 会读取响应并要求合法 JSON,故 `response` 必须是合法 JSON。
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

// ── 端到端交叉验证:被测生产签名 vs 标准 crate 重算 ─────────────────────────

/// 让真实生产代码 `send_action` 计算并发出签名,捕获线上 payload,再用标准
/// `hmac`+`sha2` crate 对相同 canonical 独立重算,断言两者相等。
///
/// 与自比对不同:此处 `production_signature` 来自被测代码本身。若生产侧改了
/// canonical 格式或 HMAC 实现,此断言会失败——这正是交叉验证要守住的。
#[test]
fn action_signature_matches_production() {
    const TOKEN: &str = "test-token";
    const ACTION_TYPE: &str = "PrefetchFile";
    const TARGET: &str = "url:https://x.test/f";
    const URGENCY: &str = "Immediate";

    let (port, rx) = spawn_mock_bridge(br#"{"status":"ok"}"#);

    // 走真实生产路径:send_action 内部计算签名并写出完整 payload。
    send_action("127.0.0.1", port, TOKEN, ACTION_TYPE, TARGET, URGENCY)
        .expect("send_action should succeed against mock bridge");

    let payload = rx
        .recv()
        .expect("mock bridge should capture the request payload");
    let v: Value = serde_json::from_str(&payload).expect("payload must be valid JSON");

    // 从生产 payload 提取时间戳与签名(时间戳由 send_action 用 wall clock 生成)。
    let issued_at_ms = v["issued_at_ms"]
        .as_i64()
        .expect("payload carries issued_at_ms");
    let expires_at_ms = v["expires_at_ms"]
        .as_i64()
        .expect("payload carries expires_at_ms");
    let action_type = v["action"]["action_type"]
        .as_str()
        .expect("payload carries action.action_type");
    let target = v["action"]["target"]
        .as_str()
        .expect("payload carries a non-null action.target");
    let urgency = v["action"]["urgency"]
        .as_str()
        .expect("payload carries action.urgency");
    let production_signature = v["action_signature"]
        .as_str()
        .expect("payload carries action_signature");

    // 用提取出的真实值,以标准 crate 独立重算。
    let recomputed = recompute_action_signature(
        TOKEN,
        issued_at_ms,
        expires_at_ms,
        action_type,
        target,
        urgency,
    );

    assert_eq!(
        recomputed, production_signature,
        "independent HMAC must equal the production action_signature"
    );
    assert_eq!(
        production_signature.len(),
        64,
        "HMAC-SHA256 hex must be 64 chars"
    );
}

// ── RFC 4231 已知向量交叉验证 ────────────────────────────────────────────────

/// RFC 4231 Test Case 1：key = 0x0b×20, message = "Hi There"。
/// 该向量同时出现在 `android_bridge` 的单测 `action_signature_matches_known_vector`
/// 中，作为手写 HMAC 实现的正确性锚点。
/// 本测试用标准 `hmac` crate 重算同一向量，交叉确认两种实现输出一致。
#[test]
fn rfc4231_case1_cross_verify() {
    let key = [0x0bu8; 20];
    let message = b"Hi There";

    // 生产侧 golden（来自 aios-cli 单测）。
    let production_golden = "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7";

    let independent = recompute_hmac(&key, message);
    assert_eq!(
        independent, production_golden,
        "standard hmac crate must match RFC 4231 case 1 golden vector"
    );
}

// ── 敏感性交叉验证 ──────────────────────────────────────────────────────────

/// 不同 token 必须产生不同 signature（HMAC 确实绑定了 key）。
#[test]
fn action_signature_is_key_sensitive() {
    let sig_a = recompute_action_signature("token-a", 1000, 2000, "NoOp", "", "Immediate");
    let sig_b = recompute_action_signature("token-b", 1000, 2000, "NoOp", "", "Immediate");
    assert_ne!(
        sig_a, sig_b,
        "different tokens must produce different HMACs"
    );
}

/// 不同 target 必须产生不同 signature（length-prefix 防止拼接歧义）。
#[test]
fn action_signature_is_target_sensitive() {
    let sig_a = recompute_action_signature(
        "token",
        1000,
        2000,
        "PrefetchFile",
        "url:https://a.test",
        "Immediate",
    );
    let sig_b = recompute_action_signature(
        "token",
        1000,
        2000,
        "PrefetchFile",
        "url:https://b.test",
        "Immediate",
    );
    assert_ne!(
        sig_a, sig_b,
        "different targets must produce different HMACs"
    );
}

/// length-prefix 防拼接歧义：`action_type="AB", target="C"` 与
/// `action_type="A", target="BC"` 的 canonical 消息不同，签名必须不同。
#[test]
fn length_prefix_prevents_concatenation_ambiguity() {
    let sig_a = recompute_action_signature("token", 1000, 2000, "AB", "C", "Immediate");
    let sig_b = recompute_action_signature("token", 1000, 2000, "A", "BC", "Immediate");
    assert_ne!(
        sig_a, sig_b,
        "length-prefix must prevent action_type/target concatenation ambiguity"
    );
}
