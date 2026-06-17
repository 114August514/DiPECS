use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

pub fn load_payload(
    json_text: Option<&str>,
    payload_file: Option<&Path>,
    prefetch_target: Option<&str>,
    auth_token: Option<&str>,
) -> Result<String> {
    let payload = match (json_text, payload_file, prefetch_target) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => {
            bail!("choose exactly one of --json, --file, or --prefetch-target")
        },
        (Some(text), None, None) => validate_and_compact(text),
        (None, Some(path), None) => {
            let text = fs::read_to_string(path)
                .with_context(|| format!("reading payload file {}", path.display()))?;
            validate_and_compact(&text)
        },
        (None, None, Some(target)) => Ok(json!({
            "intent_id": "manual-prefetch",
            "action": {
                "action_type": "PrefetchFile",
                "target": target,
                "urgency": "IdleTime"
            },
            "authorized_at_ms": 0
        })
        .to_string()),
        (None, None, None) => {
            bail!("provide one of --json, --file, or --prefetch-target")
        },
    }?;
    inject_auth_token(&payload, auth_token)
}

pub fn send_authorized_action(host: &str, port: u16, payload: &str) -> Result<()> {
    let mut stream =
        TcpStream::connect((host, port)).with_context(|| format!("connecting to {host}:{port}"))?;
    stream
        .write_all(payload.as_bytes())
        .with_context(|| format!("writing payload to {host}:{port}"))?;
    stream
        .flush()
        .with_context(|| format!("flushing payload to {host}:{port}"))?;
    Ok(())
}

fn validate_and_compact(text: &str) -> Result<String> {
    let value: Value = serde_json::from_str(text).context("payload is not valid JSON")?;
    Ok(value.to_string())
}

fn inject_auth_token(payload: &str, auth_token: Option<&str>) -> Result<String> {
    let Some(token) = auth_token else {
        return Ok(payload.to_string());
    };
    let mut value: Value = serde_json::from_str(payload).context("payload is not valid JSON")?;
    let Some(object) = value.as_object_mut() else {
        bail!("payload must be a JSON object")
    };
    object.insert("auth_token".to_string(), Value::String(token.to_string()));
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::load_payload;
    use serde_json::Value;

    #[test]
    fn build_prefetch_payload_from_target() {
        let payload =
            load_payload(None, None, Some("url:https://example.test/feed.json"), None).unwrap();
        let value: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(value["action"]["action_type"], "PrefetchFile");
        assert_eq!(
            value["action"]["target"],
            "url:https://example.test/feed.json"
        );
    }

    #[test]
    fn compact_json_payload() {
        let payload = load_payload(
            Some("{\n  \"intent_id\": \"demo\",\n  \"action\": {\"action_type\": \"NoOp\", \"urgency\": \"IdleTime\"},\n  \"authorized_at_ms\": 0\n}"),
            None,
            None,
            None,
        )
        .unwrap();

        assert!(!payload.contains('\n'));
    }

    #[test]
    fn inject_auth_token_into_json_payload() {
        let payload = load_payload(
            Some(r#"{"intent_id":"demo","action":{"action_type":"NoOp","urgency":"IdleTime"},"authorized_at_ms":0}"#),
            None,
            None,
            Some("secret-token"),
        )
        .unwrap();
        let value: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(value["auth_token"], "secret-token");
    }

    #[test]
    fn prefetch_payload_includes_auth_token() {
        let payload = load_payload(
            None,
            None,
            Some("url:https://example.test/feed.json"),
            Some("secret-token"),
        )
        .unwrap();
        let value: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(value["auth_token"], "secret-token");
        assert_eq!(value["action"]["action_type"], "PrefetchFile");
    }
}
