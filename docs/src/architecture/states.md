# 状态机

> Status: Current  
> Last verified: 2026-06-30  
> Code anchors: `aios-daemon::pipeline`, `aios-core::action_lifecycle`, `aios-spec::governance`

当前系统有两类状态：窗口处理状态和动作治理状态。早期文档中的 `Completed`、`RolledBack`、retry、budget、scheduler 等状态目前没有实现，不应作为当前行为理解。

## 窗口处理状态

Daemon 的处理循环不是一个显式 enum 状态机，但行为可按以下阶段理解：

```text
WaitRawOrDeadline
  -> SanitizeRawEvent
  -> BufferSanitizedEvent
  -> CloseWindow
  -> Decide
  -> GovernActions
  -> RecordWindow
  -> WaitRawOrDeadline
```

| 阶段 | 触发 | 主要代码 | 输出 |
| --- | --- | --- | --- |
| `WaitRawOrDeadline` | `tokio::select!` 等待 raw event / deadline | `aios-daemon/src/lib.rs` | `ProcessingEvent` |
| `SanitizeRawEvent` | 收到 `IngestedRawEvent` | `DefaultPrivacyAirGap::sanitize_with_tier` | `SanitizedEvent` |
| `BufferSanitizedEvent` | 脱敏完成 | `WindowAggregator::push` | 当前窗口 buffer |
| `CloseWindow` | deadline 或 raw channel close | `WindowAggregator::close` | `StructuredContext` |
| `Decide` | window closed | `DecisionRouter::evaluate_model_input` | `DecisionBackendResult` |
| `GovernActions` | decision returned | `ActionLifecycle::run` | `Vec<AuditRecord>` |
| `RecordWindow` | action governance finished | `RuntimeTraceRecorder` / tracing | runtime NDJSON / logs |

## 动作治理状态

每个建议动作都有确定性坐标：

```rust
ActionCoord {
    window_ordinal,
    intent_ordinal,
    action_ordinal,
}
```

每个 `ActionCoord` 必须产出恰好一条终态 `AuditRecord`。

### 成功路径

```text
Proposed
  -> SchemaValidated
  -> PolicyChecked
  -> Dispatched
  -> Succeeded
```

### Schema 拒绝

```text
Proposed
  -> RejectedInvalidSchema
```

例子：

- `PreWarmProcess` 缺少非空 target
- `PureRead` action 承载 `High` risk intent

### 策略 / 能力拒绝

```text
Proposed
  -> SchemaValidated
  -> PolicyChecked
  -> DeniedByCapability
```

或：

```text
Proposed
  -> SchemaValidated
  -> PolicyChecked
  -> DeniedByPolicy
```

`RiskExceedsCapability` 和 `ActionCapabilityDenied` 映射到 `DeniedByCapability`。其他策略拒绝映射到 `DeniedByPolicy`。

### 执行失败

```text
Proposed
  -> SchemaValidated
  -> PolicyChecked
  -> Dispatched
  -> Failed
```

`Failed` 表示 adapter 返回 `AdapterError`，不是 policy 拒绝。

## 终态

当前终态只有：

- `Succeeded`
- `RejectedInvalidSchema`
- `DeniedByCapability`
- `DeniedByPolicy`
- `Failed`

当前没有：

- `RolledBack`
- `Retrying`
- `Cancelled`
- `Expired`
- `Scheduled`
- `BudgetReserved`
- `DeniedByBudget`

这些机制如果未来实现，需要新增 RFC 和源码状态，而不是只在文档中声明。

## AuditRecord 不变量

- 每个 `ActionProposal` 恰好一条 `AuditRecord`。
- `coord` 是 replay canonical audit 的稳定主键。
- `outcome` 只在成功时写入，并使用不含 latency 的 `ActionOutcomeSummary`。
- `route`、`source_tier` 纳入审计记录，便于回放和能力边界验证。

## 相关文档

- [动作治理](action-governance.md)
- [RFC-0002 Action Bus Governance](../rfc/0002-action-bus-governance.md)
