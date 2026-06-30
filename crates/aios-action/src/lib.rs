//! # aios-action — authorized action execution layer
//!
//! Responsibility: receive `AuthorizedAction` values produced by
//! `aios_core::action_lifecycle::ActionLifecycle` and execute low-risk
//! operations behind the action boundary.
//!
//! Three sibling adapters implement `ActionAdapter`; `ActionLifecycle` injects
//! exactly one at construction:
//!
//! - [`OfflineAdapter`] — deterministic, no I/O. The canonical replay / golden
//!   adapter; the only one in the audit-hash path.
//! - [`DefaultActionExecutor`] — desktop stub. Pure, deterministic `tracing`
//!   stubs with no network or environment access.
//! - [`AndroidAdapter`] — on-device. Forwards supported actions to the Android
//!   localhost bridge over a request/response protocol and maps the device's
//!   real result back honestly. Never in the hash path.

use aios_core::governance::{ActionAdapter, AuthorizedAction};
use aios_spec::governance::{ActionOutcome, AdapterError};
use aios_spec::intent::ActionType;

pub mod android_adapter;
pub mod offline_adapter;
pub use android_adapter::{AndroidAdapter, AndroidBridgeConfig};
pub use offline_adapter::OfflineAdapter;

/// Default action executor for desktop replay.
///
/// Implements `ActionAdapter`: it can only receive an `AuthorizedAction` from
/// `ActionLifecycle`, never construct one itself. Behavior is pure and
/// deterministic — no network, no environment reads — so it is safe in the
/// golden-hash path alongside `OfflineAdapter`.
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
