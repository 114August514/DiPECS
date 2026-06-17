//! # aios-action — authorized action execution layer
//!
//! Responsibility: receive `AuthorizedAction` values approved by
//! `PolicyEngine` and execute low-risk operations behind the action boundary.
//!
//! The default executor still preserves the existing stub behavior for local
//! desktop replay. When explicitly enabled through environment variables, it
//! can also forward supported actions to the Android localhost bridge.

use std::env;
use std::io::Write;
use std::net::TcpStream;
use std::time::Instant;

use aios_spec::traits::{ActionExecutor, ActionResult};
use aios_spec::{ActionType, AuthorizedAction};
use serde_json::{to_string, to_value, Value};

const DEFAULT_ANDROID_ACTION_BRIDGE_PORT: u16 = 46321;

/// Default action executor used by replay and daemon pipelines.
pub struct DefaultActionExecutor;

impl ActionExecutor for DefaultActionExecutor {
    fn execute(&self, authorized: &AuthorizedAction) -> ActionResult {
        let start = Instant::now();
        let action = &authorized.action;
        let action_name = format!("{:?}", action.action_type);

        if let Some(config) = AndroidBridgeConfig::from_env() {
            match try_forward_to_android_bridge(authorized, &config) {
                Ok(ForwardOutcome::Forwarded) => {
                    return ActionResult {
                        action_type: action_name,
                        target: action.target.clone(),
                        success: true,
                        error: None,
                        latency_us: start.elapsed().as_micros() as u64,
                    };
                },
                Ok(ForwardOutcome::Skipped(reason)) => {
                    tracing::debug!(reason = %reason, "Android action bridge skipped");
                },
                Err(error) => {
                    return ActionResult {
                        action_type: action_name,
                        target: action.target.clone(),
                        success: false,
                        error: Some(error),
                        latency_us: start.elapsed().as_micros() as u64,
                    };
                },
            }
        }

        let (success, error) = match action.action_type {
            ActionType::PreWarmProcess => {
                if let Some(ref target) = action.target {
                    tracing::info!(
                        target = %target,
                        urgency = ?action.urgency,
                        "PreWarmProcess: stub (third-party prewarm is not implemented)"
                    );
                    (true, None)
                } else {
                    (false, Some("PreWarmProcess requires a target app".into()))
                }
            },
            ActionType::PrefetchFile => {
                tracing::info!(
                    target = ?action.target,
                    urgency = ?action.urgency,
                    "PrefetchFile: stub (local desktop fallback)"
                );
                (true, None)
            },
            ActionType::KeepAlive => {
                if let Some(ref target) = action.target {
                    tracing::info!(
                        target = %target,
                        urgency = ?action.urgency,
                        "KeepAlive: stub (Android-safe keepalive not wired here)"
                    );
                    (true, None)
                } else {
                    tracing::info!("KeepAlive: no target specified, skipping");
                    (true, None)
                }
            },
            ActionType::ReleaseMemory => {
                tracing::info!(
                    target = ?action.target,
                    urgency = ?action.urgency,
                    "ReleaseMemory: stub (Android-safe release not wired here)"
                );
                (true, None)
            },
            ActionType::NoOp => {
                tracing::debug!("NoOp executed");
                (true, None)
            },
        };

        ActionResult {
            action_type: action_name,
            target: action.target.clone(),
            success,
            error,
            latency_us: start.elapsed().as_micros() as u64,
        }
    }
}

impl Default for DefaultActionExecutor {
    fn default() -> Self {
        Self
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
    if !matches!(authorized.action.action_type, ActionType::PrefetchFile) {
        return Ok(ForwardOutcome::Skipped(
            "only PrefetchFile is currently supported by the Android bridge",
        ));
    }

    let Some(target) = authorized.action.target.as_deref() else {
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
    use super::{
        authorized_action_payload, env_flag, try_forward_to_android_bridge, AndroidBridgeConfig,
        ForwardOutcome, DEFAULT_ANDROID_ACTION_BRIDGE_PORT,
    };
    use aios_spec::{ActionType, ActionUrgency, AuthorizedAction, SuggestedAction};
    use serde_json::Value;

    fn make_action(action_type: ActionType, target: Option<&str>) -> AuthorizedAction {
        AuthorizedAction {
            intent_id: "intent-test".into(),
            action: SuggestedAction {
                action_type,
                target: target.map(|s| s.to_string()),
                urgency: ActionUrgency::Immediate,
            },
            authorized_at_ms: 1000,
        }
    }

    #[test]
    fn bridge_skips_non_prefetch_actions() {
        let config = AndroidBridgeConfig {
            host: "127.0.0.1".into(),
            port: DEFAULT_ANDROID_ACTION_BRIDGE_PORT,
            auth_token: Some("secret-token".into()),
        };
        let action = make_action(ActionType::NoOp, None);
        let result = try_forward_to_android_bridge(&action, &config).unwrap();
        assert_eq!(
            result,
            ForwardOutcome::Skipped(
                "only PrefetchFile is currently supported by the Android bridge"
            )
        );
    }

    #[test]
    fn bridge_skips_non_android_targets() {
        let config = AndroidBridgeConfig {
            host: "127.0.0.1".into(),
            port: DEFAULT_ANDROID_ACTION_BRIDGE_PORT,
            auth_token: Some("secret-token".into()),
        };
        let action = make_action(ActionType::PrefetchFile, Some("/tmp/cache.db"));
        let result = try_forward_to_android_bridge(&action, &config).unwrap();
        assert_eq!(
            result,
            ForwardOutcome::Skipped("PrefetchFile target is not an Android bridge target")
        );
    }

    #[test]
    fn env_flag_accepts_true_values() {
        assert!(env_flag_eval("true"));
        assert!(env_flag_eval("1"));
        assert!(env_flag_eval("ON"));
        assert!(!env_flag_eval("false"));
    }

    #[test]
    fn bridge_payload_includes_auth_token() {
        let action = make_action(
            ActionType::PrefetchFile,
            Some("url:https://example.test/feed.json"),
        );
        let payload = authorized_action_payload(&action, "secret-token").unwrap();
        let value: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(value["auth_token"], "secret-token");
        assert_eq!(value["action"]["action_type"], "PrefetchFile");
    }

    fn env_flag_eval(value: &str) -> bool {
        std::env::set_var("DIPECS_TEST_FLAG", value);
        let enabled = env_flag("DIPECS_TEST_FLAG");
        std::env::remove_var("DIPECS_TEST_FLAG");
        enabled
    }
}
