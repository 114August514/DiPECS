//! Action Bus 生命周期状态机
//!
//! 唯一入口 `ActionLifecycle::run`。它把每个 `(intent, action)` 展开为一个
//! `ActionProposal`，驱动其经过 schema 校验、策略裁决、adapter 执行，最终产出
//! 恰好一条带终态的 `AuditRecord`。

use std::collections::HashMap;

use aios_spec::governance::{
    ActionCoord, ActionOutcomeSummary, ActionProposal, ActionState, AuditRecord, EffectClass,
    PolicyActionDecision, PolicyVerdict,
};
use aios_spec::intent::{
    ActionType, CapabilityLevel, DecisionRoute, DenialReason, IntentBatch, RiskLevel,
};
use aios_spec::StructuredContext;
use thiserror::Error;
use tracing::debug;

use crate::governance::{ActionAdapter, AuthorizedAction};
use crate::policy_engine::PolicyEngine;

/// 生命周期状态机内部错误。这些错误不会 panic，而是被记录为对应 `AuditRecord`
/// 的 `Failed` 终态或 `RejectedInvalidSchema` 终态。
#[derive(Debug, Clone, Error)]
pub enum LifecycleError {
    #[error("internal: missing policy decision for coord {0:?}")]
    MissingPolicyDecision(ActionCoord),
    #[error("schema violation: {0}")]
    SchemaViolation(String),
}

/// Action Bus 治理状态机。
///
/// 内部持有 `PolicyEngine` 与 `ActionAdapter`，是 `AuthorizedAction` 的唯一构造点。
pub struct ActionLifecycle<'a> {
    policy: &'a PolicyEngine,
    adapter: &'a dyn ActionAdapter,
}

impl<'a> ActionLifecycle<'a> {
    pub fn new(policy: &'a PolicyEngine, adapter: &'a dyn ActionAdapter) -> Self {
        Self { policy, adapter }
    }

    /// 暴露内部持有的 `PolicyEngine`，供调用方在 Policy-only 阶段直接做策略裁决。
    pub fn policy(&self) -> &'a PolicyEngine {
        self.policy
    }

    /// 处理一个窗口的意图批次，返回每条动作恰好一条终态审计记录。
    ///
    /// `window_ordinal` 由调用方（replay/daemon 驱动循环）显式传入，保证状态机
    /// 本身无隐藏可变状态，且 replay 间坐标稳定。
    pub fn run(
        &self,
        window_ordinal: u32,
        batch: &IntentBatch,
        route: DecisionRoute,
        backend_error: Option<String>,
        capability: &CapabilityLevel,
        ctx: &StructuredContext,
    ) -> Vec<AuditRecord> {
        let policy_decisions = self
            .policy
            .evaluate_batch_with_context(batch, capability, ctx);
        let decisions_by_coord: HashMap<(u32, u32), &PolicyActionDecision> = policy_decisions
            .iter()
            .map(|d| ((d.intent_ordinal, d.action_ordinal), d))
            .collect();

        let mut records = Vec::new();
        let authorized_at_ms = batch.generated_at_ms;

        for (intent_ordinal, intent) in batch.intents.iter().enumerate() {
            let intent_ordinal = intent_ordinal as u32;
            for (action_ordinal, suggested_action) in intent.suggested_actions.iter().enumerate() {
                let action_ordinal = action_ordinal as u32;
                let coord = ActionCoord {
                    window_ordinal,
                    intent_ordinal,
                    action_ordinal,
                };
                let proposal = ActionProposal {
                    intent_id: intent.intent_id.clone(),
                    coord,
                    action: suggested_action.clone(),
                    effect: EffectClass::from_action_type(&suggested_action.action_type),
                    proposed_at_ms: authorized_at_ms,
                };

                let mut record = AuditRecord::new(&proposal, route, ctx.summary.source_tier);

                // Schema validation
                if let Some(err) = validate_schema(&proposal, &intent.risk_level) {
                    record.transition(ActionState::RejectedInvalidSchema);
                    record.error = Some(err.to_string());
                    record.backend_error = backend_error.clone();
                    records.push(record);
                    continue;
                }
                record.transition(ActionState::SchemaValidated);

                // Policy verdict lookup
                let decision = decisions_by_coord
                    .get(&(intent_ordinal, action_ordinal))
                    .copied()
                    .ok_or(LifecycleError::MissingPolicyDecision(coord));

                match decision {
                    Ok(d) => match d.verdict {
                        PolicyVerdict::Approved => {
                            record.transition(ActionState::PolicyChecked);
                            let authorized = AuthorizedAction::seal(&proposal, authorized_at_ms);
                            record.transition(ActionState::Dispatched);
                            match self.adapter.execute(&authorized) {
                                Ok(outcome) => {
                                    record.transition(ActionState::Succeeded);
                                    record.outcome =
                                        Some(ActionOutcomeSummary::from_outcome(&outcome));
                                    record.backend_error = backend_error.clone();
                                },
                                Err(err) => {
                                    record.transition(ActionState::Failed);
                                    record.error = Some(err.to_string());
                                    record.backend_error = backend_error.clone();
                                },
                            }
                        },
                        PolicyVerdict::Denied(reason) => {
                            record.transition(ActionState::PolicyChecked);
                            let terminal = denial_to_terminal(reason);
                            record.transition(terminal);
                            record.denial_reason = Some(reason);
                            record.backend_error = backend_error.clone();
                        },
                    },
                    Err(err) => {
                        debug!(error = %err, "lifecycle internal error");
                        record.transition(ActionState::Failed);
                        record.error = Some(err.to_string());
                        record.backend_error = backend_error.clone();
                    },
                }

                records.push(record);
            }
        }

        records
    }
}

/// Schema 校验：缺失必需 target、非法 risk/effect 组合等。
fn validate_schema(proposal: &ActionProposal, intent_risk: &RiskLevel) -> Option<LifecycleError> {
    if proposal.action.action_type == ActionType::PreWarmProcess {
        match proposal.action.target.as_deref() {
            Some(t) if !t.is_empty() => {},
            _ => {
                return Some(LifecycleError::SchemaViolation(
                    "PreWarmProcess requires a non-empty target".into(),
                ));
            },
        }
    }

    // risk/effect 非法组合：PureRead 动作不应承载 High 风险意图。
    if proposal.effect == EffectClass::PureRead && *intent_risk == RiskLevel::High {
        return Some(LifecycleError::SchemaViolation(
            "PureRead action cannot carry High risk intent".into(),
        ));
    }

    None
}

/// 将 `DenialReason` 映射到终态。
///
/// Capability 相关拒绝独立为 `DeniedByCapability`，其余策略拒绝归
/// `DeniedByPolicy`。
fn denial_to_terminal(reason: DenialReason) -> ActionState {
    use aios_spec::intent::DenialReason;
    match reason {
        DenialReason::RiskExceedsCapability | DenialReason::ActionCapabilityDenied => {
            ActionState::DeniedByCapability
        },
        _ => ActionState::DeniedByPolicy,
    }
}
