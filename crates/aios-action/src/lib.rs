//! # aios-action — authorized action execution layer
//!
//! Responsibility: receive `AuthorizedAction` values produced by
//! `aios_core::action_lifecycle::ActionLifecycle` and execute low-risk
//! operations behind the action boundary.
//!
//! The default executor still preserves the existing stub behavior for local
//! desktop replay. When explicitly enabled through environment variables, it
//! can also forward supported actions to the Android localhost bridge.

use std::env;
use std::io::Write;
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};

use aios_core::governance::{ActionAdapter, AuthorizedAction};
use aios_spec::governance::{ActionOutcome, AdapterError};
use aios_spec::intent::ActionType;
use serde_json::{to_string, to_value, Value};
use sha2::{Digest, Sha256};

pub mod offline_adapter;
pub use offline_adapter::OfflineAdapter;

const DEFAULT_ANDROID_ACTION_BRIDGE_PORT: u16 = 46321;
const ANDROID_ACTION_PAYLOAD_TTL_MS: i64 = 60_000;

/// Default action executor used by daemon pipeline.
///
/// Implements `ActionAdapter`: it can only receive `AuthorizedAction` from
/// `ActionLifecycle`, never construct one itself.
pub struct DefaultActionExecutor;

impl DefaultActionExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultActionExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionAdapter for DefaultActionExecutor {
    fn name(&self) -> &'static str {
        "default"
    }

    fn execute(&self, authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError> {
        let action = authorized.action();
        let action_name = format!("{:?}", action.action_type);

        if let Some(config) = AndroidBridgeConfig::from_env() {
            match try_forward_to_android_bridge(authorized, &config) {
                Ok(ForwardOutcome::Forwarded) => {
                    return Ok(ActionOutcome {
                        action_type: action_name,
                        target: action.target.clone(),
                        summary: "forwarded_to_android_bridge".into(),
                        latency_us: 0,
                    });
                },
                Ok(ForwardOutcome::Skipped(reason)) => {
                    tracing::debug!(reason = %reason, "Android action bridge skipped");
                },
                Err(error) => {
                    return Err(AdapterError::AndroidBridgeError(error));
                },
            }
        }

        let summary = match action.action_type {
            ActionType::PreWarmProcess => match action.target.as_deref() {
                Some(target) => {
                    tracing::info!(
                        target = %target,
                        urgency = ?action.urgency,
                        "PreWarmProcess: stub (third-party prewarm is not implemented)"
                    );
                    "stub_prewarm".to_string()
                },
                None => {
                    return Err(AdapterError::ExecutionError(
                        "PreWarmProcess requires a target app".into(),
                    ));
                },
            },
            ActionType::PrefetchFile => {
                tracing::info!(
                    target = ?action.target,
                    urgency = ?action.urgency,
                    "PrefetchFile: stub (local desktop fallback)"
                );
                "stub_prefetch".to_string()
            },
            ActionType::KeepAlive => {
                if let Some(ref target) = action.target {
                    tracing::info!(
                        target = %target,
                        urgency = ?action.urgency,
                        "KeepAlive: stub (Android-safe keepalive not wired here)"
                    );
                    format!("stub_keepalive:{target}")
                } else {
                    tracing::info!("KeepAlive: no target specified, skipping");
                    "stub_keepalive:system".to_string()
                }
            },
            ActionType::ReleaseMemory => {
                tracing::info!(
                    target = ?action.target,
                    urgency = ?action.urgency,
                    "ReleaseMemory: stub (Android-safe release not wired here)"
                );
                "stub_release_memory".to_string()
            },
            ActionType::NoOp => {
                tracing::debug!("NoOp executed");
                "noop".to_string()
            },
        };

        Ok(ActionOutcome {
            action_type: action_name,
            target: action.target.clone(),
            summary,
            latency_us: 0,
        })
    }
}

#[derive(Debug, Clone)]
struct AndroidBridgeConfig {
    host: String,
    port: u16,
    auth_token: Option<String>,
}

impl AndroidBridgeConfig {
    fn from_env() -> Option<Self> {
        if !env_flag("DIPECS_ANDROID_ACTION_BRIDGE_ENABLED") {
            return None;
        }

        let host = env::var("DIPECS_ANDROID_ACTION_BRIDGE_HOST")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port = env::var("DIPECS_ANDROID_ACTION_BRIDGE_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(DEFAULT_ANDROID_ACTION_BRIDGE_PORT);
        let auth_token = env::var("DIPECS_ANDROID_ACTION_BRIDGE_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        Some(Self {
            host,
            port,
            auth_token,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ForwardOutcome {
    Forwarded,
    Skipped(&'static str),
}

fn try_forward_to_android_bridge(
    authorized: &AuthorizedAction,
    config: &AndroidBridgeConfig,
) -> Result<ForwardOutcome, String> {
    let action = authorized.action();
    if let Some(reason) = android_bridge_skip_reason(&action.action_type, action.target.as_deref())
    {
        return Ok(ForwardOutcome::Skipped(reason));
    }

    let Some(auth_token) = config.auth_token.as_deref() else {
        return Err(
            "DIPECS_ANDROID_ACTION_BRIDGE_TOKEN is required when forwarding to Android bridge"
                .into(),
        );
    };

    let payload = authorized_action_payload(authorized, auth_token)?;
    let mut stream = TcpStream::connect((&*config.host, config.port)).map_err(|error| {
        format!(
            "connect Android action bridge {}:{}: {error}",
            config.host, config.port
        )
    })?;
    stream.write_all(payload.as_bytes()).map_err(|error| {
        format!(
            "write AuthorizedAction to Android bridge {}:{}: {error}",
            config.host, config.port
        )
    })?;
    stream.flush().map_err(|error| {
        format!(
            "flush AuthorizedAction to Android bridge {}:{}: {error}",
            config.host, config.port
        )
    })?;

    tracing::info!(
        host = %config.host,
        port = config.port,
        target = ?action.target,
        "Forwarded AuthorizedAction to Android bridge"
    );
    Ok(ForwardOutcome::Forwarded)
}

fn android_bridge_skip_reason(
    action_type: &ActionType,
    target: Option<&str>,
) -> Option<&'static str> {
    match action_type {
        ActionType::PrefetchFile => match target {
            Some(value) if value.starts_with("url:") || value.starts_with("uri:") => None,
            Some(_) => Some("PrefetchFile target is not an Android bridge target"),
            None => Some("PrefetchFile without target keeps local stub behavior"),
        },
        ActionType::KeepAlive => match target {
            Some(value) if value.starts_with("work:") => None,
            None => None,
            Some(_) => Some("KeepAlive target is not DiPECS-owned work"),
        },
        ActionType::ReleaseMemory => match target {
            Some("cache:prefetch" | "cache:all") => None,
            None => None,
            Some(_) => Some("ReleaseMemory target is not app-owned cache"),
        },
        ActionType::PreWarmProcess => match target {
            Some(value)
                if value.starts_with("own:")
                    || value.starts_with("notif:")
                    || value.starts_with("pkg:") =>
            {
                None
            },
            Some(_) => Some("PreWarmProcess target is not Android-safe"),
            None => Some("PreWarmProcess without target keeps local stub behavior"),
        },
        ActionType::NoOp => Some("NoOp keeps local stub behavior"),
    }
}

fn authorized_action_payload(
    authorized: &AuthorizedAction,
    auth_token: &str,
) -> Result<String, String> {
    let mut value = to_value(authorized)
        .map_err(|error| format!("serialize AuthorizedAction for Android bridge: {error}"))?;
    let Some(object) = value.as_object_mut() else {
        return Err("serialized AuthorizedAction was not a JSON object".into());
    };
    object.insert(
        "auth_token".to_string(),
        Value::String(auth_token.to_string()),
    );
    let issued_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("calculate Android bridge payload issue time: {error}"))?
        .as_millis() as i64;
    object.insert(
        "issued_at_ms".to_string(),
        Value::Number(issued_at_ms.into()),
    );
    object.insert(
        "expires_at_ms".to_string(),
        Value::Number((issued_at_ms + ANDROID_ACTION_PAYLOAD_TTL_MS).into()),
    );
    object.insert(
        "action_signature".to_string(),
        Value::String(action_signature(
            auth_token,
            issued_at_ms,
            issued_at_ms + ANDROID_ACTION_PAYLOAD_TTL_MS,
            authorized,
        )),
    );
    to_string(&value)
        .map_err(|error| format!("serialize authenticated Android bridge payload: {error}"))
}

fn action_signature(
    auth_token: &str,
    issued_at_ms: i64,
    expires_at_ms: i64,
    authorized: &AuthorizedAction,
) -> String {
    let action = authorized.action();
    let target = action.target.as_deref().unwrap_or("");
    let canonical = canonical_action_signature_input(
        issued_at_ms,
        expires_at_ms,
        &format!("{:?}", action.action_type),
        target,
        &format!("{:?}", action.urgency),
    );
    hmac_sha256_hex(auth_token.as_bytes(), canonical.as_bytes())
}

fn canonical_action_signature_input(
    issued_at_ms: i64,
    expires_at_ms: i64,
    action_type: &str,
    target: &str,
    urgency: &str,
) -> String {
    format!(
        "dipecs.android.action.v1\nissued_at_ms:{issued_at_ms}\nexpires_at_ms:{expires_at_ms}\naction_type:{}:{action_type}\ntarget:{}:{target}\nurgency:{}:{urgency}",
        action_type.len(),
        target.len(),
        urgency.len(),
    )
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

fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

#[cfg(test)]
mod tests {
    use super::{
        android_bridge_skip_reason, canonical_action_signature_input, env_flag, hmac_sha256_hex,
    };
    use aios_spec::intent::ActionType;

    #[test]
    fn env_flag_accepts_true_values() {
        assert!(env_flag_eval("true"));
        assert!(env_flag_eval("1"));
        assert!(env_flag_eval("ON"));
        assert!(!env_flag_eval("false"));
    }

    fn env_flag_eval(value: &str) -> bool {
        std::env::set_var("DIPECS_TEST_FLAG", value);
        let enabled = env_flag("DIPECS_TEST_FLAG");
        std::env::remove_var("DIPECS_TEST_FLAG");
        enabled
    }

    #[test]
    fn hmac_sha256_matches_rfc4231_case_1() {
        let key = [0x0b; 20];
        let signature = hmac_sha256_hex(&key, b"Hi There");

        assert_eq!(
            signature,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
        );
    }

    #[test]
    fn canonical_signature_input_is_length_prefixed() {
        let input = canonical_action_signature_input(
            1000,
            2000,
            "PrefetchFile",
            "url:https://example.test/a:b",
            "Immediate",
        );

        assert!(input.contains("action_type:12:PrefetchFile"));
        assert!(input.contains("target:28:url:https://example.test/a:b"));
        assert!(input.contains("urgency:9:Immediate"));
    }

    #[test]
    fn android_bridge_allows_only_safe_action_targets() {
        assert_eq!(
            android_bridge_skip_reason(&ActionType::PrefetchFile, Some("url:https://example.test")),
            None,
        );
        assert_eq!(
            android_bridge_skip_reason(&ActionType::KeepAlive, Some("work:collector_heartbeat")),
            None,
        );
        assert_eq!(
            android_bridge_skip_reason(&ActionType::ReleaseMemory, Some("cache:prefetch")),
            None,
        );
        assert_eq!(
            android_bridge_skip_reason(&ActionType::PreWarmProcess, Some("own:resources")),
            None,
        );
        assert_eq!(
            android_bridge_skip_reason(&ActionType::PreWarmProcess, Some("pkg:com.example")),
            None,
        );
        assert!(android_bridge_skip_reason(&ActionType::KeepAlive, Some("com.example")).is_some());
        assert!(
            android_bridge_skip_reason(&ActionType::ReleaseMemory, Some("cache:other")).is_some()
        );
    }
}
