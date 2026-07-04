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
mod tests;
