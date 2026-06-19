use std::io::Write;
use std::net::TcpStream;

use anyhow::{Context, Result};
use serde_json::json;

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
        .write_all(payload.as_bytes())
        .with_context(|| format!("writing ping to {host}:{port}"))?;
    stream
        .flush()
        .with_context(|| format!("flushing ping to {host}:{port}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
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
}
