//! 策略引擎 — 校验 LLM 返回的意图是否合法
//!
//! 职责:
//! 1. 检查风险等级是否允许自动执行 (引擎配置 + 后端能力双重)
//! 2. 检查推荐的 action 是否在白名单内
//! 3. 检查目标 app 是否可操作（必须在本窗口的上下文中出现过）
//! 4. 输出逐 action 的裁决 (`PolicyActionDecision`)，不再构造 `AuthorizedAction`

use std::collections::BTreeSet;

use aios_spec::governance::{PolicyActionDecision, PolicyVerdict};
use aios_spec::intent::{
    ActionType, ActionUrgency, CapabilityLevel, DenialReason, Intent, IntentBatch, RiskLevel,
};
use aios_spec::{SanitizedEventType, StructuredContext};
use tracing::debug;

/// 策略引擎配置
#[derive(Debug, Clone)]
pub struct PolicyConfig {
    /// 允许自动执行的最大风险等级
    pub max_auto_risk: RiskLevel,
    /// 禁止的 action 类型 (按 Debug 名称子串匹配)
    pub blocked_actions: Vec<String>,
    /// 单次最多执行的动作数
    pub max_actions_per_batch: usize,
    /// 置信度下限——低于此值的意图直接拒绝
    pub min_confidence: f32,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            max_auto_risk: RiskLevel::Low,
            blocked_actions: vec![],
            max_actions_per_batch: 5,
            min_confidence: 0.3,
        }
    }
}

/// 策略引擎
pub struct PolicyEngine {
    config: PolicyConfig,
}

impl PolicyEngine {
    pub fn new(config: PolicyConfig) -> Self {
        Self { config }
    }

    /// 校验整个 IntentBatch, 返回每个建议动作的策略裁决。
    ///
    /// 不检查后端能力等级，也不进行 target-not-in-context 检查——
    /// 用于未指定后端 / 未携带窗口上下文的场景或向后兼容。
    pub fn evaluate_batch(&self, batch: &IntentBatch) -> Vec<PolicyActionDecision> {
        let capability = CapabilityLevel::for_route(aios_spec::DecisionRoute::Mock);
        let mut decisions = Vec::new();
        for (intent_ordinal, intent) in batch.intents.iter().enumerate() {
            let intent_ordinal = intent_ordinal as u32;
            decisions.extend(self.evaluate_intent(intent, intent_ordinal, &capability, None));
        }
        decisions
    }

    /// 校验整个 IntentBatch，同时执行后端能力等级检查。
    pub fn evaluate_batch_with_capability(
        &self,
        batch: &IntentBatch,
        capability: &CapabilityLevel,
    ) -> Vec<PolicyActionDecision> {
        let mut decisions = Vec::new();
        for (intent_ordinal, intent) in batch.intents.iter().enumerate() {
            let intent_ordinal = intent_ordinal as u32;
            decisions.extend(self.evaluate_intent(intent, intent_ordinal, capability, None));
        }
        decisions
    }

    /// 完整的校验入口：风险 + 能力 + 上下文。
    ///
    /// 当上下文 (`ctx`) 提供时，引擎会拒绝任何 `target` 指向未在本窗口
    /// 出现过的 package/path 的动作。这是封堵 LLM 凭空指定目标的关键门。
    pub fn evaluate_batch_with_context(
        &self,
        batch: &IntentBatch,
        capability: &CapabilityLevel,
        ctx: &StructuredContext,
    ) -> Vec<PolicyActionDecision> {
        let known = Some(KnownTargets::from_context(ctx));
        let mut decisions = Vec::new();
        for (intent_ordinal, intent) in batch.intents.iter().enumerate() {
            let intent_ordinal = intent_ordinal as u32;
            decisions.extend(self.evaluate_intent(
                intent,
                intent_ordinal,
                capability,
                known.as_ref(),
            ));
        }
        decisions
    }

    fn evaluate_intent(
        &self,
        intent: &Intent,
        intent_ordinal: u32,
        capability: &CapabilityLevel,
        known: Option<&KnownTargets>,
    ) -> Vec<PolicyActionDecision> {
        // 1. 后端能力等级检查 — 先于通用策略
        if !capability.allows_risk(intent.risk_level) {
            return self.denied_all(
                intent,
                intent_ordinal,
                DenialReason::RiskExceedsCapability,
                "risk exceeds backend capability",
            );
        }

        // 2. 通用风险等级检查
        if intent.risk_level as u8 > self.config.max_auto_risk as u8 {
            return self.denied_all(
                intent,
                intent_ordinal,
                DenialReason::RiskExceedsConfig,
                "risk exceeds engine config",
            );
        }

        // 3. 置信度下限
        if intent.confidence < self.config.min_confidence {
            return self.denied_all(
                intent,
                intent_ordinal,
                DenialReason::ConfidenceTooLow,
                "confidence below floor",
            );
        }

        // 4. 过滤动作，逐 action 产出裁决
        let mut decisions = Vec::new();
        let mut approved_count = 0usize;

        for (action_ordinal, action) in intent.suggested_actions.iter().enumerate() {
            let action_ordinal = action_ordinal as u32;

            if approved_count >= self.config.max_actions_per_batch {
                debug!(
                    intent_id = %intent.intent_id,
                    reason = ?DenialReason::BatchActionCapExceeded,
                    "policy denial"
                );
                decisions.push(PolicyActionDecision {
                    intent_ordinal,
                    action_ordinal,
                    verdict: PolicyVerdict::Denied(DenialReason::BatchActionCapExceeded),
                });
                continue;
            }

            let action_name = format!("{:?}", action.action_type);
            if self
                .config
                .blocked_actions
                .iter()
                .any(|blocked| action_name.contains(blocked))
            {
                debug!(
                    intent_id = %intent.intent_id,
                    reason = ?DenialReason::ActionTypeBlocked,
                    action = %action_name,
                    "policy denial"
                );
                decisions.push(PolicyActionDecision {
                    intent_ordinal,
                    action_ordinal,
                    verdict: PolicyVerdict::Denied(DenialReason::ActionTypeBlocked),
                });
                continue;
            }

            if matches!(action.urgency, ActionUrgency::Deferred) {
                debug!(
                    intent_id = %intent.intent_id,
                    reason = ?DenialReason::ActionUrgencyDeferred,
                    "policy denial"
                );
                decisions.push(PolicyActionDecision {
                    intent_ordinal,
                    action_ordinal,
                    verdict: PolicyVerdict::Denied(DenialReason::ActionUrgencyDeferred),
                });
                continue;
            }

            if !capability.allows_action(&action.action_type) {
                debug!(
                    intent_id = %intent.intent_id,
                    reason = ?DenialReason::ActionCapabilityDenied,
                    action = %action_name,
                    "policy denial"
                );
                decisions.push(PolicyActionDecision {
                    intent_ordinal,
                    action_ordinal,
                    verdict: PolicyVerdict::Denied(DenialReason::ActionCapabilityDenied),
                });
                continue;
            }

            if let Some(k) = known {
                if let Some(reason) = check_target(&action.action_type, action.target.as_deref(), k)
                {
                    debug!(
                        intent_id = %intent.intent_id,
                        reason = ?reason,
                        target = ?action.target,
                        "policy denial"
                    );
                    decisions.push(PolicyActionDecision {
                        intent_ordinal,
                        action_ordinal,
                        verdict: PolicyVerdict::Denied(reason),
                    });
                    continue;
                }
            }

            approved_count += 1;
            decisions.push(PolicyActionDecision {
                intent_ordinal,
                action_ordinal,
                verdict: PolicyVerdict::Approved,
            });
        }

        decisions
    }

    fn denied_all(
        &self,
        intent: &Intent,
        intent_ordinal: u32,
        reason: DenialReason,
        log_msg: &'static str,
    ) -> Vec<PolicyActionDecision> {
        debug!(
            intent_id = %intent.intent_id,
            reason = ?reason,
            "policy denial: {log_msg}"
        );
        (0..intent.suggested_actions.len())
            .map(|action_ordinal| PolicyActionDecision {
                intent_ordinal,
                action_ordinal: action_ordinal as u32,
                verdict: PolicyVerdict::Denied(reason),
            })
            .collect()
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new(PolicyConfig::default())
    }
}

/// 检查 (action_type, target) 是否通过 target 校验。返回 Some(reason)
/// 即拒绝。规则：
///
/// - `NoOp` — 不关心 target。
/// - `PreWarmProcess` — executor 强制要求 target；None 视为 hallucinated。
/// - `KeepAlive` / `ReleaseMemory` / `PrefetchFile` — None 是合法的"系统/窗口
///   范围"语义；如果给了 Some(target) 则必须在 KnownTargets 中。
///
/// 这一切都是为了拦截 LLM 凭空指定 package：宁可拒绝一条可疑动作，
/// 也不让从未在窗口里出现过的实体被执行器接受。
fn check_target(
    action: &ActionType,
    target: Option<&str>,
    known: &KnownTargets,
) -> Option<DenialReason> {
    match action {
        ActionType::NoOp => None,
        ActionType::PreWarmProcess => match target {
            Some(t) if !t.is_empty() && (known.packages.contains(t) || known.files.contains(t)) => {
                None
            },
            _ => Some(DenialReason::TargetNotInContext),
        },
        ActionType::KeepAlive | ActionType::ReleaseMemory | ActionType::PrefetchFile => {
            match target {
                None => None,
                Some("") => Some(DenialReason::TargetNotInContext),
                Some(t) if known.packages.contains(t) || known.files.contains(t) => None,
                Some(_) => Some(DenialReason::TargetNotInContext),
            }
        },
    }
}

/// 一次窗口上下文中"已知"的可操作实体。
///
/// 由 [`StructuredContext`] 派生：所有出现在 sanitized 事件里的 package
/// 名以及 ContextSummary 汇总里的 foreground/notified apps。当前不收集
/// 具体文件路径（脱敏阶段已经丢弃），但保留 `files` 字段以便未来扩展。
struct KnownTargets {
    packages: BTreeSet<String>,
    files: BTreeSet<String>,
}

impl KnownTargets {
    fn from_context(ctx: &StructuredContext) -> Self {
        let mut packages: BTreeSet<String> = BTreeSet::new();
        for pkg in &ctx.summary.foreground_apps {
            packages.insert(pkg.clone());
        }
        for pkg in &ctx.summary.notified_apps {
            packages.insert(pkg.clone());
        }
        for event in &ctx.events {
            if let Some(pkg) = event.app_package.as_ref() {
                packages.insert(pkg.clone());
            }
            match &event.event_type {
                SanitizedEventType::AppTransition { package_name, .. } => {
                    packages.insert(package_name.clone());
                },
                SanitizedEventType::Notification { source_package, .. } => {
                    packages.insert(source_package.clone());
                },
                SanitizedEventType::ProcessResource {
                    package_name: Some(p),
                    ..
                } => {
                    packages.insert(p.clone());
                },
                _ => {},
            }
        }
        Self {
            packages,
            files: BTreeSet::new(),
        }
    }
}
