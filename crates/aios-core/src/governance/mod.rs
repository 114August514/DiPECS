//! 动作治理凭证与 dispatch 抽象
//!
//! `AuthorizedAction` 是 Action Bus 上唯一可被执行的凭证。它字段私有，构造器
//! `pub(crate) seal` 只允许同 crate 的 `ActionLifecycle` 在策略通过后调用，从而
//! 在编译期保证：任何外部 crate（包括 `aios-action`）无法伪造或反序列化出一个
//! 可执行动作。

use aios_spec::governance::{
    ActionCoord, ActionOutcome, ActionProposal, AdapterError, EffectClass,
};
use aios_spec::intent::SuggestedAction;

/// 经 `ActionLifecycle` 审查通过、允许交给 adapter 执行的动作凭证。
///
/// - 字段私有，外部 crate 无法 struct-literal 构造。
/// - 不实现 `Deserialize`，防止反序列化伪造。
/// - 只暴露 getter，adapter 只能读取不能修改。
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthorizedAction {
    intent_id: String,
    coord: ActionCoord,
    action: SuggestedAction,
    effect: EffectClass,
    authorized_at_ms: i64,
}

impl AuthorizedAction {
    /// 唯一构造器。仅 `crate::action_lifecycle` 可在 `PolicyChecked` 后调用。
    pub(crate) fn seal(proposal: &ActionProposal, authorized_at_ms: i64) -> Self {
        Self {
            intent_id: proposal.intent_id.clone(),
            coord: proposal.coord,
            action: proposal.action.clone(),
            effect: proposal.effect,
            authorized_at_ms,
        }
    }

    pub fn intent_id(&self) -> &str {
        &self.intent_id
    }

    pub fn coord(&self) -> ActionCoord {
        self.coord
    }

    pub fn action(&self) -> &SuggestedAction {
        &self.action
    }

    pub fn effect(&self) -> EffectClass {
        self.effect
    }

    pub fn authorized_at_ms(&self) -> i64 {
        self.authorized_at_ms
    }
}

/// 动作 dispatch 抽象。
///
/// `DefaultActionExecutor`（Android bridge 转发 + 本地 stub）与
/// `OfflineAdapter`（纯离线确定性模拟）都实现此 trait，由 `ActionLifecycle`
/// 在运行时二选一注入。
pub trait ActionAdapter {
    fn name(&self) -> &'static str;
    fn execute(&self, authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use aios_spec::governance::ActionCoord;
    use aios_spec::intent::{ActionType, ActionUrgency};

    #[test]
    fn seal_copies_proposal_fields() {
        let proposal = ActionProposal {
            intent_id: "intent-1".into(),
            coord: ActionCoord {
                window_ordinal: 1,
                intent_ordinal: 2,
                action_ordinal: 3,
            },
            action: SuggestedAction {
                action_type: ActionType::NoOp,
                target: None,
                urgency: ActionUrgency::Immediate,
            },
            effect: EffectClass::PureRead,
            proposed_at_ms: 1000,
        };

        let authorized = AuthorizedAction::seal(&proposal, 2000);
        assert_eq!(authorized.intent_id(), "intent-1");
        assert_eq!(authorized.coord(), proposal.coord);
        assert!(matches!(authorized.action().action_type, ActionType::NoOp));
        assert_eq!(authorized.effect(), EffectClass::PureRead);
        assert_eq!(authorized.authorized_at_ms(), 2000);
    }
}
