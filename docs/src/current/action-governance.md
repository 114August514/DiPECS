# 动作治理

> Status: Current  
> Last verified: 2026-06-30

动作治理的核心不变量是：**决策后端只能提出 `SuggestedAction`，真正可执行的 `AuthorizedAction` 只能由 `ActionLifecycle` 在策略通过后构造。**

## 类型边界

```text
IntentBatch
  -> Intent
  -> SuggestedAction        # 不可信建议
  -> ActionProposal         # core 生成坐标和 effect
  -> PolicyActionDecision   # PolicyEngine 逐 action 裁决
  -> AuthorizedAction       # ActionLifecycle 唯一 seal 点
  -> ActionAdapter
  -> AuditRecord
```

`AuthorizedAction` 定义在 `aios-core::governance`，字段私有。`aios-action` 只能接收引用并读取 getter，不能自行构造授权动作。

## PolicyEngine 检查

当前策略检查覆盖：

- 后端能力等级：`CapabilityLevel::for_route(route)`
- 全局自动执行风险上限：默认只允许 `Low`
- 置信度下限：默认 `0.3`
- blocked action 子串
- `Deferred` urgency 拒绝
- 单 intent action 数量上限
- action 是否在后端能力白名单内
- target 是否出现在当前 `StructuredContext` 中

常见拒绝原因由 `DenialReason` 表示，例如 `RiskExceedsCapability`、`ActionCapabilityDenied`、`TargetNotInContext`。

## ActionLifecycle 状态机

每个 `(window_ordinal, intent_ordinal, action_ordinal)` 形成一个确定性 `ActionCoord`。每个 coord 恰好产出一条终态 `AuditRecord`。

当前可达状态：

```text
Proposed
  -> SchemaValidated
  -> PolicyChecked
  -> Dispatched
  -> Succeeded

Proposed
  -> RejectedInvalidSchema

Proposed
  -> SchemaValidated
  -> PolicyChecked
  -> DeniedByCapability | DeniedByPolicy

Proposed
  -> SchemaValidated
  -> PolicyChecked
  -> Dispatched
  -> Failed
```

当前没有 retry、rollback、budget reservation、scheduler state 或 cancel/expire 状态；这些机制不存在，因此文档和代码都不应声明对应终态。

## AuditRecord

`AuditRecord` 是动作治理的审计输出，包含：

- `coord`
- `action_type`
- `target`
- `effect`
- `route`
- `source_tier`
- `transitions`
- `terminal`
- `outcome`
- `denial_reason`
- `backend_error`
- `error`

Replay 的 canonical projection 会剥离 UUID、latency 等 volatile 字段，把确定性审计流折叠进 `audit_hash`。

## 执行适配器

| Adapter | 使用场景 | 行为 |
| --- | --- | --- |
| `DefaultActionExecutor` | daemon 在线路径 | 默认本地 stub；可通过 env 转发 Android-safe action 到 Android socket。 |
| `OfflineAdapter` | replay / golden tests | 不访问系统、网络或 Android，只返回确定性 `ActionOutcome`。 |

`DefaultActionExecutor` 只有在 `DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=true` 且 token 配置完整时才尝试转发 Android action。否则会走本地 stub summary。
