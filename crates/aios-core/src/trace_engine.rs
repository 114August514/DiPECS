//! Trace 引擎 — 确定性回放验证
//!
//! 记录 `GoldenTrace` 并在回放时验证脱敏 / 策略 / 执行的确定性。
//!
//! 设计取舍：
//! - 脱敏校验完全在 `aios-core` 内完成 (sanitizer 是 `PrivacyAirGap` 的
//!   trait 对象)，所以 `validate_sanitization` 不需要外部依赖。
//! - 策略与执行的"实际值"必须由调用方驱动 (`aios-cli` 或 daemon 已经
//!   持有 router/policy/executor)，否则 `aios-core` 就要反向依赖
//!   `aios-agent`。`TraceValidator::validate` 因此接收调用方已经计算好的
//!   `actual_intents` 与 `actual_executed`，引擎只负责按"语义"逐条比对
//!   并填出 `ReplayResult`。
//!
//! 语义比对刻意忽略易变字段 (uuids、wall-clock 时间)，所以回放只验证
//! pipeline 的可观测结果，不验证 token-by-token 的字节一致。如果想锁
//! 字节一致，请用 `ReplaySummary.audit_hash`。

use aios_spec::traits::{PrivacySanitizer, TraceValidator};
use aios_spec::{
    ExecutedAction, GoldenTrace, Intent, IntentBatch, ReplayResult, SanitizedEvent, SourceTier,
    SuggestedAction,
};

/// 默认 Trace 引擎
pub struct DefaultTraceEngine {
    sanitizer: Box<dyn PrivacySanitizer + Send + Sync>,
}

impl DefaultTraceEngine {
    pub fn new(sanitizer: impl PrivacySanitizer + Send + Sync + 'static) -> Self {
        Self {
            sanitizer: Box::new(sanitizer),
        }
    }

    /// 仅校验脱敏。`policy_match` 与 `execution_match` 显式置为 `false`，
    /// 且对应的 divergence 列表为空 —— 含义是"这一层未被检查"，而不是
    /// "检查过且失败"。两层语义分离：
    ///
    /// - match flag 回答"该层是否通过校验"。
    /// - divergence 列表回答"如果失败了，是哪里失败"。
    ///
    /// 调用方通过 `result.all_match()` 自然区分；想做端到端校验请改用
    /// [`TraceValidator::validate`]，它会把三层都填出来。
    pub fn validate_sanitization(&self, golden: &GoldenTrace) -> ReplayResult {
        let sanitization_divergences = self.sanitization_divergences(golden);
        ReplayResult {
            trace_id: golden.trace_id.clone(),
            sanitization_match: sanitization_divergences.is_empty(),
            sanitization_divergences,
            policy_match: false,
            policy_divergences: vec![],
            execution_match: false,
            execution_divergences: vec![],
        }
    }

    fn sanitization_divergences(&self, golden: &GoldenTrace) -> Vec<usize> {
        let default_tier = SourceTier::PublicApi;
        let tiers = golden
            .source_tiers
            .iter()
            .chain(std::iter::repeat(&default_tier));

        let actual_sanitized: Vec<SanitizedEvent> = golden
            .raw_events
            .iter()
            .zip(tiers)
            .map(|(raw, tier)| self.sanitizer.sanitize_with_tier(raw.clone(), *tier))
            .collect();

        let mut divergences = Vec::new();
        for (i, (actual, expected)) in actual_sanitized
            .iter()
            .zip(golden.expected_sanitized.iter())
            .enumerate()
        {
            if !sanitized_eq(actual, expected) {
                divergences.push(i);
            }
        }
        // 长度不一致时，把多余/缺失的索引也算进去。
        let actual_len = actual_sanitized.len();
        let expected_len = golden.expected_sanitized.len();
        for i in actual_len.min(expected_len)..actual_len.max(expected_len) {
            divergences.push(i);
        }
        divergences
    }
}

impl TraceValidator for DefaultTraceEngine {
    fn validate(
        &self,
        golden: &GoldenTrace,
        actual_intents: &IntentBatch,
        actual_executed: &[ExecutedAction],
    ) -> ReplayResult {
        let sanitization_divergences = self.sanitization_divergences(golden);
        let policy_divergences = intent_divergences(&golden.expected_intents, actual_intents);
        let execution_divergences =
            execution_divergences(&golden.expected_actions, actual_executed);

        ReplayResult {
            trace_id: golden.trace_id.clone(),
            sanitization_match: sanitization_divergences.is_empty(),
            sanitization_divergences,
            policy_match: policy_divergences.is_empty(),
            policy_divergences,
            execution_match: execution_divergences.is_empty(),
            execution_divergences,
        }
    }
}

// ============================================================
// 语义比较 — 忽略易变字段
// ============================================================

/// 比较两个 SanitizedEvent 的语义内容是否一致。
///
/// event_id 和 timestamp_ms 不在比较范围内 (它们因生成时间不同而变化)。
fn sanitized_eq(a: &SanitizedEvent, b: &SanitizedEvent) -> bool {
    a.event_type == b.event_type
        && a.source_tier == b.source_tier
        && a.app_package == b.app_package
        && a.uid == b.uid
}

/// 比较两个 IntentBatch 的语义内容：window_id / generated_at_ms / intent_id
/// 因为是 uuid/时间戳被刻意忽略。其它所有字段（包括 `rationale_tags`）都
/// 参与比较。
fn intent_divergences(expected: &IntentBatch, actual: &IntentBatch) -> Vec<String> {
    let mut divergences = Vec::new();

    if expected.model != actual.model {
        divergences.push(format!(
            "model mismatch: expected={:?} actual={:?}",
            expected.model, actual.model
        ));
    }
    if expected.intents.len() != actual.intents.len() {
        divergences.push(format!(
            "intent count mismatch: expected={} actual={}",
            expected.intents.len(),
            actual.intents.len()
        ));
        // 长度不同时继续按最小公共前缀逐条比对，便于定位首个差异。
    }
    let pairs = expected
        .intents
        .iter()
        .zip(actual.intents.iter())
        .enumerate();
    for (i, (e, a)) in pairs {
        if let Some(reason) = intent_diff(e, a) {
            divergences.push(format!("intent[{i}]: {reason}"));
        }
    }
    divergences
}

fn intent_diff(expected: &Intent, actual: &Intent) -> Option<String> {
    if expected.intent_type != actual.intent_type {
        return Some(format!(
            "intent_type: expected={:?} actual={:?}",
            expected.intent_type, actual.intent_type
        ));
    }
    if expected.risk_level != actual.risk_level {
        return Some(format!(
            "risk_level: expected={:?} actual={:?}",
            expected.risk_level, actual.risk_level
        ));
    }
    if (expected.confidence - actual.confidence).abs() > f32::EPSILON {
        return Some(format!(
            "confidence: expected={} actual={}",
            expected.confidence, actual.confidence
        ));
    }
    if expected.rationale_tags != actual.rationale_tags {
        return Some(format!(
            "rationale_tags: expected={:?} actual={:?}",
            expected.rationale_tags, actual.rationale_tags
        ));
    }
    if expected.suggested_actions.len() != actual.suggested_actions.len() {
        return Some(format!(
            "suggested_actions count: expected={} actual={}",
            expected.suggested_actions.len(),
            actual.suggested_actions.len()
        ));
    }
    for (j, (e_act, a_act)) in expected
        .suggested_actions
        .iter()
        .zip(actual.suggested_actions.iter())
        .enumerate()
    {
        if !suggested_eq(e_act, a_act) {
            return Some(format!(
                "suggested_actions[{j}]: expected={:?} actual={:?}",
                e_act, a_act
            ));
        }
    }
    None
}

fn suggested_eq(a: &SuggestedAction, b: &SuggestedAction) -> bool {
    a.action_type == b.action_type && a.target == b.target && a.urgency == b.urgency
}

fn execution_divergences(expected: &[ExecutedAction], actual: &[ExecutedAction]) -> Vec<usize> {
    let mut divergences = Vec::new();
    for (i, (e, a)) in expected.iter().zip(actual.iter()).enumerate() {
        if !executed_eq(e, a) {
            divergences.push(i);
        }
    }
    // 长度差异也记入索引。
    let actual_len = actual.len();
    let expected_len = expected.len();
    for i in actual_len.min(expected_len)..actual_len.max(expected_len) {
        divergences.push(i);
    }
    divergences
}

/// 比较两个 ExecutedAction：忽略 executed_at_ms。
fn executed_eq(a: &ExecutedAction, b: &ExecutedAction) -> bool {
    a.action_type == b.action_type
        && a.target == b.target
        && a.success == b.success
        && a.error_reason == b.error_reason
}
