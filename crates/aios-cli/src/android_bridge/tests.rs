use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::thread;
use std::time::Duration;

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

/// Read until a complete JSON value, like Android's `readPayload`, then reply.
fn read_valid_json_then_reply(listener: TcpListener, response: &[u8]) {
    let response = response.to_vec();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => panic!("payload ended before a complete JSON value"),
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if std::str::from_utf8(&buf)
                        .ok()
                        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                        .is_some()
                    {
                        break;
                    }
                },
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

    read_valid_json_then_reply(listener, br#"{"status":"ok","message":"pong"}"#);

    send_ping("127.0.0.1", port, "secret").unwrap();
}

#[test]
fn ping_rejects_non_ok_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    read_valid_json_then_reply(listener, br#"{"status":"forbidden"}"#);

    let err = send_ping("127.0.0.1", port, "secret").unwrap_err();
    assert!(err.to_string().contains("forbidden"));
}

#[test]
fn ping_keeps_write_side_open_until_bridge_replies() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let saw_eof_before_reply = std::sync::Arc::new(std::sync::Mutex::new(false));
    let saw_eof_clone = saw_eof_before_reply.clone();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            assert_ne!(n, 0, "ping payload must arrive before EOF");
            buf.extend_from_slice(&chunk[..n]);
            let text = std::str::from_utf8(&buf).unwrap();
            if serde_json::from_str::<serde_json::Value>(text).is_ok() {
                break;
            }
        }

        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut probe = [0u8; 1];
        match stream.read(&mut probe) {
            Ok(0) => *saw_eof_clone.lock().unwrap() = true,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut => {},
            Ok(n) => panic!("unexpected extra {n} bytes after complete JSON"),
            Err(error) => panic!("unexpected probe read error: {error}"),
        }

        stream
            .write_all(br#"{"status":"ok","message":"pong"}"#)
            .unwrap();
        stream.flush().unwrap();
        stream.shutdown(Shutdown::Write).ok();
    });

    send_ping("127.0.0.1", port, "secret").unwrap();
    assert!(
        !*saw_eof_before_reply.lock().unwrap(),
        "ping must not half-close before the bridge can reply"
    );
    handle.join().unwrap();
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
