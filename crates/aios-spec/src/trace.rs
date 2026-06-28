//! 确定性 Trace — "how we prove correctness"
//!
//! Golden Trace 是 DiPECS 的确定性保证机制:
//! 在相同输入序列下, 系统的脱敏输出和策略决策必须一致。

use serde::{Deserialize, Serialize};

use crate::event::{RawEvent, SourceTier};
use crate::intent::IntentBatch;
use crate::SanitizedEvent;

fn default_source_tiers() -> Vec<SourceTier> {
    Vec::new()
}

/// 一条 Golden Trace
///
/// 记录特定时间窗口内:
/// 1. 输入序列 (RawEvent)
/// 2. 期望的脱敏输出 (SanitizedEvent)
/// 3. 期望的 LLM 返回 (IntentBatch)
/// 4. 期望的执行动作 (ExecutedAction)
///
/// Golden Trace 文件存储在 `data/traces/` 目录下。
/// 超过 1MB 的 trace 文件使用 Git LFS 托管。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenTrace {
    /// Trace 唯一 ID
    pub trace_id: String,
    /// 窗口起始时间 (epoch ms)
    pub window_start_ms: i64,
    /// 窗口结束时间 (epoch ms)
    pub window_end_ms: i64,
    /// 原始输入事件序列
    pub raw_events: Vec<RawEvent>,
    /// 期望的脱敏输出
    pub expected_sanitized: Vec<SanitizedEvent>,
    /// 期望的云端返回
    pub expected_intents: IntentBatch,
    /// 期望的本地执行动作
    pub expected_actions: Vec<ExecutedAction>,
    /// Per-event source tier, parallel to `raw_events`.
    /// Empty or missing entries are handled at the consumption site
    /// (trace_engine falls back to `SourceTier::PublicApi`).
    #[serde(default = "default_source_tiers")]
    pub source_tiers: Vec<SourceTier>,
}

/// 已执行的动作记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutedAction {
    /// 动作类型
    pub action_type: String,
    /// 目标标识
    pub target: Option<String>,
    /// 执行时间 (epoch ms)
    pub executed_at_ms: i64,
    /// 执行是否成功
    pub success: bool,
    /// 失败原因 (如有)
    pub error_reason: Option<String>,
}

/// 回放验证结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    /// 对应的 trace ID
    pub trace_id: String,
    /// 脱敏输出是否完全一致
    pub sanitization_match: bool,
    /// 不一致的 SanitizedEvent 索引
    pub sanitization_divergences: Vec<usize>,
    /// 策略决策是否完全一致
    pub policy_match: bool,
    /// 不一致的策略决策描述
    pub policy_divergences: Vec<String>,
    /// 执行结果是否完全一致
    pub execution_match: bool,
    /// 不一致的 ExecutedAction 索引
    pub execution_divergences: Vec<usize>,
}

impl ReplayResult {
    /// True iff sanitize + policy + execute all matched. The single boolean
    /// that golden tests should pin against.
    pub fn all_match(&self) -> bool {
        self.sanitization_match && self.policy_match && self.execution_match
    }
}
