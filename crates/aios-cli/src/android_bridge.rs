use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::json;

const READ_TIMEOUT_MS: u64 = 5000;
const MAX_RESPONSE_BYTES: usize = 4096;

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

    let mut buf = [0u8; MAX_RESPONSE_BYTES];
    let n = stream
        .read(&mut buf)
        .with_context(|| format!("reading pong from {host}:{port}"))?;
    let text =
        std::str::from_utf8(&buf[..n]).with_context(|| "bridge returned non-UTF-8 response")?;
    let value: serde_json::Value =
        serde_json::from_str(text).with_context(|| "bridge returned invalid JSON")?;

    match value.get("status").and_then(|v| v.as_str()) {
        Some("ok") => Ok(()),
        Some(other) => bail!("bridge returned status '{other}'"),
        None => bail!("bridge response missing status field"),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::send_ping;

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

    #[test]
    fn ping_validates_ok_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).unwrap();
            let req = std::str::from_utf8(&buf[..n]).unwrap();
            let value: serde_json::Value = serde_json::from_str(req).unwrap();
            assert_eq!(value["message_type"], "ping");
            stream
                .write_all(br#"{"status":"ok","message":"pong"}"#)
                .unwrap();
            stream.flush().unwrap();
        });

        send_ping("127.0.0.1", port, "secret").unwrap();
    }

    #[test]
    fn ping_rejects_non_ok_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).unwrap();
            stream.write_all(br#"{"status":"forbidden"}"#).unwrap();
            stream.flush().unwrap();
        });

        let err = send_ping("127.0.0.1", port, "secret").unwrap_err();
        assert!(err.to_string().contains("forbidden"));
    }
}
