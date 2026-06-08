use crate::intent::IntentBatch;
use crate::trace::{ExecutedAction, GoldenTrace, ReplayResult};

/// Trace 验证器
///
/// 给定相同的 `RawEvent` 输入序列以及对一次完整回放采集到的
/// `actual_intents` 和 `actual_executed`，验证：
/// 1. 脱敏输出是否逐条一致 (`PrivacyAirGap` 的确定性)
/// 2. 策略意图是否一致 (`PolicyEngine` + `DecisionRouter` 的确定性)
/// 3. 执行动作是否一致 (`ActionExecutor` 的确定性)
///
/// 调用方负责驱动 pipeline 取得 actual_*；验证器只做语义比对。
/// 这样 trace 验证器留在 `aios-spec/core` 边界内，不必反向依赖
/// `aios-agent` 这样的高层组件。
pub trait TraceValidator {
    /// 对比 Golden Trace，返回三个维度的验证结果
    fn validate(
        &self,
        golden: &GoldenTrace,
        actual_intents: &IntentBatch,
        actual_executed: &[ExecutedAction],
    ) -> ReplayResult;
}
