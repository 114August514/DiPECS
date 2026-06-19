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

use aios_core::governance::{ActionAdapter, AuthorizedAction};
use aios_spec::governance::{ActionOutcome, AdapterError};
use aios_spec::intent::ActionType;
use serde_json::{to_string, to_value, Value};

pub mod offline_adapter;
pub use offline_adapter::OfflineAdapter;

const DEFAULT_ANDROID_ACTION_BRIDGE_PORT: u16 = 46321;

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
    if !matches!(authorized.action().action_type, ActionType::PrefetchFile) {
        return Ok(ForwardOutcome::Skipped(
            "only PrefetchFile is currently supported by the Android bridge",
        ));
    }

    let Some(target) = authorized.action().target.as_deref() else {
        return Ok(ForwardOutcome::Skipped(
            "PrefetchFile without target keeps local stub behavior",
        ));
    };

    if !(target.starts_with("url:") || target.starts_with("uri:")) {
        return Ok(ForwardOutcome::Skipped(
            "PrefetchFile target is not an Android bridge target",
        ));
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
        target = %target,
        "Forwarded AuthorizedAction to Android bridge"
    );
    Ok(ForwardOutcome::Forwarded)
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
    to_string(&value)
        .map_err(|error| format!("serialize authenticated Android bridge payload: {error}"))
}

fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

#[cfg(test)]
mod tests {
    use super::env_flag;

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
}
