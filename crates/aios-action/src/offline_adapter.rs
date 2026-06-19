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
            success: true,
            summary,
            latency_us: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aios_core::governance::AuthorizedAction;
    use aios_spec::governance::{ActionCoord, ActionOutcomeSummary, ActionProposal, EffectClass};
    use aios_spec::intent::{ActionType, ActionUrgency, SuggestedAction};

    fn make_authorized(action_type: ActionType, target: Option<&str>) -> AuthorizedAction {
        let effect = EffectClass::from_action_type(&action_type);
        let proposal = ActionProposal {
            intent_id: "intent-1".into(),
            coord: ActionCoord {
                window_ordinal: 0,
                intent_ordinal: 0,
                action_ordinal: 0,
            },
            action: SuggestedAction {
                action_type,
                target: target.map(|s| s.to_string()),
                urgency: ActionUrgency::Immediate,
            },
            effect,
            proposed_at_ms: 1000,
        };
        AuthorizedAction::seal_for_test(&proposal, 2000)
    }

    #[test]
    fn offline_adapter_covers_all_action_types() {
        let adapter = OfflineAdapter;
        let cases = vec![
            (ActionType::NoOp, None),
            (ActionType::PreWarmProcess, Some("com.example.app")),
            (
                ActionType::PrefetchFile,
                Some("url:https://example.test/feed.json"),
            ),
            (ActionType::KeepAlive, Some("com.example.app")),
            (ActionType::ReleaseMemory, None),
        ];

        for (action_type, target) in cases {
            let authorized = make_authorized(action_type, target);
            let outcome = adapter.execute(&authorized).unwrap();
            assert!(outcome.success);
            assert_eq!(outcome.latency_us, 0);
            assert!(!outcome.summary.is_empty());
        }
    }

    #[test]
    fn offline_outcome_summary_is_deterministic() {
        let adapter = OfflineAdapter;
        let authorized = make_authorized(ActionType::PrefetchFile, Some("url:https://x.test/"));
        let a = adapter.execute(&authorized).unwrap();
        let b = adapter.execute(&authorized).unwrap();
        assert_eq!(
            ActionOutcomeSummary::from_outcome(&a),
            ActionOutcomeSummary::from_outcome(&b)
        );
    }
}
