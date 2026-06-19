//! 动作治理协议类型
//!
//! 本模块定义 Action Bus 治理边界上的协议数据：不可信的 `ActionProposal`、
//! 确定性坐标 `ActionCoord`、副作用分级 `EffectClass`、生命周期状态
//! `ActionState`、审计记录 `AuditRecord` 以及裁决/执行相关的 outcome/error 类型。
//!
//! 注意：真正“可执行”的凭证 `AuthorizedAction` 不在这里定义——它必须和
//! 唯一能够构造它的状态机同处一个 crate，以便由编译器保证不可伪造性。

use serde::{Deserialize, Serialize};

use crate::intent::{ActionType, DenialReason, SuggestedAction};

/// 确定性动作坐标。
///
/// 不含 UUID、wall-clock 等运行时随机量，是单次 replay/run 内的 canonical
/// 主键。`window_ordinal` 由驱动循环按窗口处理顺序赋号，用于消除跨窗口的
/// `(intent_ordinal, action_ordinal)` 碰撞。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ActionCoord {
    pub window_ordinal: u32,
    pub intent_ordinal: u32,
    pub action_ordinal: u32,
}

/// 动作副作用分级（按当前 5 个动作的真实副作用裁剪，不照搬设计文档 6 级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EffectClass {
    /// 纯读/无状态变更，例如 NoOp。
    PureRead,
    /// 本地缓存写，例如 PrefetchFile。
    LocalCacheWrite,
    /// 本地进程/系统状态变更，例如 PreWarmProcess、KeepAlive、ReleaseMemory。
    LocalStateChange,
}

impl EffectClass {
    /// 按 `ActionType` 推导其副作用分级。
    pub fn from_action_type(action_type: &ActionType) -> Self {
        match action_type {
            ActionType::NoOp => Self::PureRead,
            ActionType::PrefetchFile => Self::LocalCacheWrite,
            ActionType::PreWarmProcess | ActionType::KeepAlive | ActionType::ReleaseMemory => {
                Self::LocalStateChange
            },
        }
    }
}

/// 不可信侧治理 envelope。
///
/// core 在边界处为来自 planner 的 `SuggestedAction` 建立此 envelope：填入确定
/// 性坐标、由可信侧推导 effect。裹在里面的 `action` 字段本身仍是不可信输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionProposal {
    /// 运行时关联用（含随机 UUID），不进 canonical hash。
    pub intent_id: String,
    /// 确定性坐标，进 canonical hash。
    pub coord: ActionCoord,
    /// 来自 planner 的不可信建议动作。
    pub action: SuggestedAction,
    /// 由 core 可信侧推导填入的副作用分级，非外部输入。
    pub effect: EffectClass,
    /// 提议时间（epoch ms），运行时 volatile，不进 canonical hash。
    pub proposed_at_ms: i64,
}

/// 动作生命周期状态（精简版，9 态）。
///
/// 只保留当前管线可达的状态；budget/调度/privacy 相关终态延后到机制存在时。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActionState {
    // 正常路径
    Proposed,
    SchemaValidated,
    PolicyChecked,
    Dispatched,
    /// 终态：执行成功。
    Succeeded,
    // 拒绝/失败终态
    /// 终态：schema 校验失败（如缺失必需 target、非法 risk/effect 组合）。
    RejectedInvalidSchema,
    /// 终态：能力等级拒绝（`PolicyEngine` 的 RiskExceedsCapability /
    /// ActionCapabilityDenied）。
    DeniedByCapability,
    /// 终态：策略拒绝（风险超配置、置信度过低、target 不在上下文等）。
    DeniedByPolicy,
    /// 终态：adapter 执行返回 Err。
    Failed,
}

impl ActionState {
    /// 是否为终态。
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::RejectedInvalidSchema
                | Self::DeniedByCapability
                | Self::DeniedByPolicy
                | Self::Failed
        )
    }
}

/// 动作执行结果（adapter 返回）。
///
/// `Ok(ActionOutcome)` 即表示执行成功；失败通过 `Result::Err(AdapterError)`
/// 表达。本结构体可能包含运行时 volatile 字段（如实际 latency），因此不能
/// 直接进 hash；需先投影为 `ActionOutcomeSummary`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOutcome {
    pub action_type: String,
    pub target: Option<String>,
    pub summary: String,
    pub latency_us: u64,
}

/// 确定性 outcome 摘要，不含 wall-clock / 随机，纳入 `audit_hash`。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ActionOutcomeSummary {
    pub action_type: String,
    pub target: Option<String>,
    pub summary: String,
}

impl ActionOutcomeSummary {
    /// 从 `ActionOutcome` 投影为确定性摘要。
    pub fn from_outcome(outcome: &ActionOutcome) -> Self {
        Self {
            action_type: outcome.action_type.clone(),
            target: outcome.target.clone(),
            summary: outcome.summary.clone(),
        }
    }
}

/// Adapter 执行错误。
///
/// 注意：不设置 `Unsupported` variant。`ActionType` 是封闭枚举且 adapter 全覆盖，
/// 未知 action_type 在更早的 schema/反序列化阶段就被拒，归 `RejectedInvalidSchema`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum AdapterError {
    #[error("simulated resource unavailable: {0}")]
    SimulatedResourceUnavailable(String),
    #[error("android bridge error: {0}")]
    AndroidBridgeError(String),
    #[error("execution error: {0}")]
    ExecutionError(String),
}

/// 策略裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyVerdict {
    Approved,
    Denied(DenialReason),
}

/// PolicyEngine 对单条建议动作的逐 action 裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyActionDecision {
    pub intent_ordinal: u32,
    pub action_ordinal: u32,
    pub verdict: PolicyVerdict,
}

/// 审计记录。
///
/// 每个 `ActionProposal`（即每个 `ActionCoord`）在生命周期中产出恰好一条。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    /// 确定性主键，进 canonical hash。
    pub coord: ActionCoord,
    /// 运行时关联（含 UUID），volatile，不进 canonical hash。
    pub intent_id: String,
    pub action_type: ActionType,
    pub target: Option<String>,
    pub effect: EffectClass,
    /// 完整迁移序列。
    pub transitions: Vec<ActionState>,
    /// 终态（冗余但便于查询/golden）。
    pub terminal: ActionState,
    /// 成功时写入 adapter outcome 的确定性摘要，进 canonical hash。
    pub outcome: Option<ActionOutcomeSummary>,
    /// 拒绝原因（终态为拒绝时）。
    pub denial_reason: Option<DenialReason>,
    /// 错误信息（终态为 Failed 时）。
    pub error: Option<String>,
}

impl AuditRecord {
    /// 从初态 `Proposed` 开始构建一条审计记录。
    pub fn new(proposal: &ActionProposal) -> Self {
        Self {
            coord: proposal.coord,
            intent_id: proposal.intent_id.clone(),
            action_type: proposal.action.action_type.clone(),
            target: proposal.action.target.clone(),
            effect: proposal.effect,
            transitions: vec![ActionState::Proposed],
            terminal: ActionState::Proposed,
            outcome: None,
            denial_reason: None,
            error: None,
        }
    }

    /// 追加一个状态迁移，并更新 `terminal`。
    pub fn transition(&mut self, state: ActionState) {
        self.transitions.push(state);
        self.terminal = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_state_terminal_detection() {
        assert!(!ActionState::Proposed.is_terminal());
        assert!(!ActionState::SchemaValidated.is_terminal());
        assert!(!ActionState::PolicyChecked.is_terminal());
        assert!(!ActionState::Dispatched.is_terminal());
        assert!(ActionState::Succeeded.is_terminal());
        assert!(ActionState::RejectedInvalidSchema.is_terminal());
        assert!(ActionState::DeniedByCapability.is_terminal());
        assert!(ActionState::DeniedByPolicy.is_terminal());
        assert!(ActionState::Failed.is_terminal());
    }

    #[test]
    fn effect_class_derived_from_action_type() {
        assert_eq!(
            EffectClass::from_action_type(&ActionType::NoOp),
            EffectClass::PureRead
        );
        assert_eq!(
            EffectClass::from_action_type(&ActionType::PrefetchFile),
            EffectClass::LocalCacheWrite
        );
        assert_eq!(
            EffectClass::from_action_type(&ActionType::PreWarmProcess),
            EffectClass::LocalStateChange
        );
        assert_eq!(
            EffectClass::from_action_type(&ActionType::KeepAlive),
            EffectClass::LocalStateChange
        );
        assert_eq!(
            EffectClass::from_action_type(&ActionType::ReleaseMemory),
            EffectClass::LocalStateChange
        );
    }
}
