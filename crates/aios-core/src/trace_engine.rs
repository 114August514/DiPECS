//! Trace 引擎 — 确定性回放验证
//!
//! 记录 `GoldenTrace` 并在回放时验证脱敏 / 策略 / 执行的确定性。
//!
//! 设计取舍：
//! - 脱敏校验完全在 aios-core 内完成 (sanitizer 是 PrivacyAirGap 的 trait
//!   对象)，所以 `validate` 不需要外部依赖。
//! - 策略与执行的"实际值"必须由调用方驱动 (aios-cli 或 daemon 已经持有
//!   router/policy/executor)，否则 aios-core 就要反向依赖 aios-agent。
//!   `validate_full` 因此接收调用方已经计算好的 `actual_intents` 和
//!   `actual_executed`，引擎只负责按"语义"逐条比对并填出 ReplayResult。
//!
//! 语义比对刻意忽略易变字段 (uuids、wall-clock 时间)，所以回放只验证
//! pipeline 的可观测结果，不验证 token-by-token 的字节一致。如果想锁字节
//! 一致，请用 `ReplaySummary.audit_hash`。

use aios_spec::traits::PrivacySanitizer;
use aios_spec::traits::TraceValidator;
use aios_spec::{
    ExecutedAction, GoldenTrace, Intent, IntentBatch, ReplayResult, SanitizedEvent, SuggestedAction,
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

    /// 全链路验证：脱敏 + 策略 + 执行。
    ///
    /// `actual_intents` / `actual_executed` 由调用方驱动完整 pipeline
    /// 后填入；引擎不知道 router / policy / executor 的存在。
    pub fn validate_full(
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

    fn sanitization_divergences(&self, golden: &GoldenTrace) -> Vec<usize> {
        let actual_sanitized: Vec<SanitizedEvent> = golden
            .raw_events
            .iter()
            .map(|raw| self.sanitizer.sanitize(raw.clone()))
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
    /// 仅校验脱敏。要校验策略 + 执行，请用 [`DefaultTraceEngine::validate_full`]。
    fn validate(&self, golden: &GoldenTrace) -> ReplayResult {
        let sanitization_divergences = self.sanitization_divergences(golden);
        ReplayResult {
            trace_id: golden.trace_id.clone(),
            sanitization_match: sanitization_divergences.is_empty(),
            sanitization_divergences,
            // 没有 actual_intents / actual_executed 输入，无从校验：
            // 用空 divergences + match=true 表达"未做检查"。Caller
            // 想要严格保证应改用 validate_full。
            policy_match: true,
            policy_divergences: vec![],
            execution_match: true,
            execution_divergences: vec![],
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
/// 因为是 uuid/时间戳被刻意忽略。
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
    // 用 Debug 字符串语义比较，避免要求 IntentType 等内部类型实现 Eq。
    if format!("{:?}", expected.intent_type) != format!("{:?}", actual.intent_type) {
        return Some(format!(
            "intent_type: expected={:?} actual={:?}",
            expected.intent_type, actual.intent_type
        ));
    }
    if !risk_eq(&expected.risk_level, &actual.risk_level) {
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

fn risk_eq(a: &aios_spec::RiskLevel, b: &aios_spec::RiskLevel) -> bool {
    *a as u8 == *b as u8
}

fn suggested_eq(a: &SuggestedAction, b: &SuggestedAction) -> bool {
    a.action_type == b.action_type
        && a.target == b.target
        && format!("{:?}", a.urgency) == format!("{:?}", b.urgency)
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
