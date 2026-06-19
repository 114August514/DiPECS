//! 纯离线、确定性的 `ActionAdapter` 实现。
//!
//! `OfflineAdapter` 不访问真实系统 / 网络 / Android，只返回确定性的
//! `ActionOutcome`。它用于 replay、测试和 golden hash 生成。

use aios_core::governance::{ActionAdapter, AuthorizedAction};
use aios_spec::governance::{ActionOutcome, AdapterError};
use aios_spec::intent::ActionType;

/// 离线模拟 adapter。
#[derive(Debug, Clone, Copy, Default)]
pub struct OfflineAdapter;

impl OfflineAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl ActionAdapter for OfflineAdapter {
    fn name(&self) -> &'static str {
        "offline"
    }

    fn execute(&self, authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError> {
        let action = authorized.action();
        let action_type_name = format!("{:?}", action.action_type);
        let target = action.target.clone();

        let summary = match action.action_type {
            ActionType::NoOp => "noop".to_string(),
            ActionType::PreWarmProcess => {
                format!(
                    "simulate_prewarm:{}",
                    target.as_deref().unwrap_or("unknown")
                )
            },
            ActionType::PrefetchFile => {
                format!("simulate_cache:{}", target.as_deref().unwrap_or("unknown"))
            },
            ActionType::KeepAlive => {
                format!(
                    "simulate_keepalive:{}",
                    target.as_deref().unwrap_or("system")
                )
            },
            ActionType::ReleaseMemory => {
                format!("simulate_release:{}", target.as_deref().unwrap_or("system"))
            },
        };

        Ok(ActionOutcome {
            action_type: action_type_name,
            target,
            summary,
            latency_us: 0,
        })
    }
}
