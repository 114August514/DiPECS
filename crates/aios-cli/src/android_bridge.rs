use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};

const READ_TIMEOUT_MS: u64 = 5000;
const MAX_RESPONSE_BYTES: usize = 4096;
const ACTION_PAYLOAD_TTL_MS: i64 = 60_000;

/// Send a ping/health-check message to the Android localhost action bridge.
///
/// The bridge is expected to reply with a JSON object containing at least
/// `"status": "ok"`. This command intentionally does **not** dispatch any
/// action; it only verifies reachability and token acceptance.
pub fn send_ping(host: &str, port: u16, auth_token: &str) -> Result<()> {
    let payload = json!({
        "message_type": "ping",
        "auth_token": auth_token,
    })
    .to_string();

    let mut stream =
        TcpStream::connect((host, port)).with_context(|| format!("connecting to {host}:{port}"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(READ_TIMEOUT_MS)))
        .with_context(|| "setting read timeout")?;
    stream
        .write_all(payload.as_bytes())
        .with_context(|| format!("writing ping to {host}:{port}"))?;
    stream
        .flush()
        .with_context(|| format!("flushing ping to {host}:{port}"))?;

    // Signal EOF so the server (which reads until EOF) knows the request is
    // complete and can send its pong without waiting for a half-open timeout.
    stream
        .shutdown(Shutdown::Write)
        .with_context(|| format!("shutting down write side to {host}:{port}"))?;

    let mut buf = Vec::with_capacity(MAX_RESPONSE_BYTES);
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() + n > MAX_RESPONSE_BYTES {
                    bail!("bridge response exceeded {MAX_RESPONSE_BYTES} bytes");
                }
                buf.extend_from_slice(&chunk[..n]);
                // If we already have a complete JSON value we can stop reading;
                // the server may keep its half-open socket alive after sending.
                if std::str::from_utf8(&buf)
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                    .is_some()
                {
                    break;
                }
            },
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if buf.is_empty() {
                    return Err(e).with_context(|| format!("reading pong from {host}:{port}"));
                }
                break;
            },
            Err(e) => {
                return Err(e).with_context(|| format!("reading pong from {host}:{port}"));
            },
        }
    }

    let text = std::str::from_utf8(&buf).with_context(|| "bridge returned non-UTF-8 response")?;
    let value: serde_json::Value =
        serde_json::from_str(text).with_context(|| "bridge returned invalid JSON")?;

    match value.get("status").and_then(|v| v.as_str()) {
        Some("ok") => Ok(()),
        Some(other) => bail!("bridge returned status '{other}'"),
        None => bail!("bridge response missing status field"),
    }
}

/// Send a real authorized action payload to the Android action bridge.
///
/// This constructs a payload that mirrors what `aios-action` produces
/// when forwarding an `AuthorizedAction` to the Android bridge, including
/// the length-prefixed canonical HMAC-SHA256 signature.
pub fn send_action(
    host: &str,
    port: u16,
    auth_token: &str,
    action_type: &str,
    target: &str,
    urgency: &str,
) -> Result<()> {
    let issued_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before epoch")?
        .as_millis() as i64;
    let expires_at_ms = issued_at_ms + ACTION_PAYLOAD_TTL_MS;
    let signature = action_signature(
        auth_token,
        issued_at_ms,
        expires_at_ms,
        action_type,
        target,
        urgency,
    );

    let target_value: serde_json::Value = if target.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(target.to_string())
    };

    let payload = json!({
        "intent_id": "cli-manual-action",
        "coord": {
            "window_ordinal": 0,
            "intent_ordinal": 0,
            "action_ordinal": 0,
        },
        "action": {
            "action_type": action_type,
            "target": target_value,
            "urgency": urgency,
        },
        "effect": "PureRead",
        "authorized_at_ms": issued_at_ms,
        "auth_token": auth_token,
        "issued_at_ms": issued_at_ms,
        "expires_at_ms": expires_at_ms,
        "action_signature": signature,
    })
    .to_string();

    let mut stream =
        TcpStream::connect((host, port)).with_context(|| format!("connecting to {host}:{port}"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(READ_TIMEOUT_MS)))
        .with_context(|| "setting read timeout")?;
    stream
        .write_all(payload.as_bytes())
        .with_context(|| format!("writing action payload to {host}:{port}"))?;
    stream
        .flush()
        .with_context(|| format!("flushing action payload to {host}:{port}"))?;

    stream
        .shutdown(Shutdown::Write)
        .with_context(|| format!("shutting down write side to {host}:{port}"))?;

    let mut buf = Vec::with_capacity(MAX_RESPONSE_BYTES);
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() + n > MAX_RESPONSE_BYTES {
                    bail!("bridge response exceeded {MAX_RESPONSE_BYTES} bytes");
                }
                buf.extend_from_slice(&chunk[..n]);
                if std::str::from_utf8(&buf)
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                    .is_some()
                {
                    break;
                }
            },
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if !buf.is_empty() {
                    break;
                }
                return Err(e)
                    .with_context(|| format!("reading bridge response from {host}:{port}"));
            },
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("reading bridge response from {host}:{port}"));
            },
        }
    }

    let text = std::str::from_utf8(&buf).with_context(|| "bridge returned non-UTF-8 response")?;
    tracing::info!(response = %text, "action sent to Android bridge");
    Ok(())
}

fn action_signature(
    auth_token: &str,
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
    hmac_sha256_hex(auth_token.as_bytes(), canonical.as_bytes())
}

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let digest = Sha256::digest(key);
        key_block[..digest.len()].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut outer_key_pad = [0x5cu8; BLOCK_SIZE];
    let mut inner_key_pad = [0x36u8; BLOCK_SIZE];
    for index in 0..BLOCK_SIZE {
        outer_key_pad[index] ^= key_block[index];
        inner_key_pad[index] ^= key_block[index];
    }

    let mut inner = Sha256::new();
    inner.update(inner_key_pad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_key_pad);
    outer.update(inner_digest);
    hex_encode(&outer.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener};
    use std::thread;

    use super::{action_signature, send_action, send_ping};

    #[test]
    fn ping_payload_is_valid_json() {
        let payload = serde_json::json!({
            "message_type": "ping",
            "auth_token": "secret",
        })
        .to_string();
        let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(value["message_type"], "ping");
        assert_eq!(value["auth_token"], "secret");
    }

    /// Read until EOF, like Android's `readPayload`, then reply.
    fn read_until_eof_then_reply(listener: TcpListener, response: &[u8]) {
        let response = response.to_vec();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
            }
            let req = std::str::from_utf8(&buf).unwrap();
            let value: serde_json::Value = serde_json::from_str(req).unwrap();
            assert_eq!(value["message_type"], "ping");
            stream.write_all(&response).unwrap();
            stream.flush().unwrap();
            stream.shutdown(Shutdown::Write).ok();
        });
    }

    #[test]
    fn ping_validates_ok_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        read_until_eof_then_reply(listener, br#"{"status":"ok","message":"pong"}"#);

        send_ping("127.0.0.1", port, "secret").unwrap();
    }

    #[test]
    fn ping_rejects_non_ok_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        read_until_eof_then_reply(listener, br#"{"status":"forbidden"}"#);

        let err = send_ping("127.0.0.1", port, "secret").unwrap_err();
        assert!(err.to_string().contains("forbidden"));
    }

    #[test]
    fn action_signature_matches_known_vector() {
        // HMAC-SHA256 test vector from RFC 4231 case 1.
        let key = [0x0bu8; 20];
        let message = b"Hi There";
        let hex = super::hmac_sha256_hex(&key, message);
        assert_eq!(
            hex,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn action_signature_is_deterministic() {
        let a = action_signature(
            "token",
            1000,
            2000,
            "PrefetchFile",
            "url:https://x.test/f",
            "Immediate",
        );
        let b = action_signature(
            "token",
            1000,
            2000,
            "PrefetchFile",
            "url:https://x.test/f",
            "Immediate",
        );
        assert_eq!(a, b);
    }

    #[test]
    fn action_signature_changes_with_different_token() {
        let a = action_signature("token-a", 1000, 2000, "NoOp", "", "Immediate");
        let b = action_signature("token-b", 1000, 2000, "NoOp", "", "Immediate");
        assert_ne!(a, b);
    }

    #[test]
    fn action_signature_changes_with_different_target() {
        let a = action_signature(
            "token",
            1000,
            2000,
            "PrefetchFile",
            "url:https://a.test",
            "Immediate",
        );
        let b = action_signature(
            "token",
            1000,
            2000,
            "PrefetchFile",
            "url:https://b.test",
            "Immediate",
        );
        assert_ne!(a, b);
    }

    #[test]
    fn send_action_includes_auth_token_and_signature() {
        // 验证 send_action 发出的 payload 必须包含 auth_token 和 action_signature,
        // 且两者随 token 变化而变化——这是 Android bridge 鉴权的基础。
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let received_clone = received.clone();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
            }
            *received_clone.lock().unwrap() = buf;
            stream.write_all(br#"{"status":"ok"}"#).ok();
            stream.flush().ok();
            stream.shutdown(Shutdown::Write).ok();
        });

        send_action(
            "127.0.0.1",
            port,
            "cli-test-token",
            "PrefetchFile",
            "url:https://example.test/f",
            "Immediate",
        )
        .unwrap();

        let buf = received.lock().unwrap();
        let text = std::str::from_utf8(&buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();

        assert_eq!(
            parsed["auth_token"].as_str().unwrap(),
            "cli-test-token",
            "payload must carry auth_token"
        );
        let sig = parsed["action_signature"].as_str().unwrap();
        assert!(!sig.is_empty(), "action_signature must not be empty");

        // 不同 token 必须产生不同 signature(即 HMAC 确实绑定了 token)。
        let alt_sig = action_signature(
            "different-token",
            parsed["issued_at_ms"].as_i64().unwrap(),
            parsed["expires_at_ms"].as_i64().unwrap(),
            "PrefetchFile",
            "url:https://example.test/f",
            "Immediate",
        );
        assert_ne!(sig, alt_sig, "signature must be token-sensitive");
    }

    #[test]
    fn send_action_empty_token_is_documented() {
        // 当前实现允许空 token:payload 中 auth_token 为空,signature 用空 key 计算。
        // 本测试把该行为钉死,以便未来若加入"拒绝空 token"校验时有明确回归基线。
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let received_clone = received.clone();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
            }
            *received_clone.lock().unwrap() = buf;
            stream.write_all(br#"{"status":"ok"}"#).ok();
            stream.flush().ok();
            stream.shutdown(Shutdown::Write).ok();
        });

        send_action("127.0.0.1", port, "", "NoOp", "", "IdleTime").unwrap();

        let buf = received.lock().unwrap();
        let text = std::str::from_utf8(&buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["auth_token"].as_str().unwrap(), "");
        assert!(!parsed["action_signature"].as_str().unwrap().is_empty());
    }
}
