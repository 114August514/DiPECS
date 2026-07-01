# 策略规则参考

> Status: Current  
> Last verified: 2026-07-01  > Code anchors: `crates/aios-core/src/policy_engine.rs`, `crates/aios-core/src/action_lifecycle.rs`, `crates/aios-spec/src/intent.rs`

**这篇文档回答什么**：`PolicyEngine` 审查每个建议动作的具体规则、`CapabilityLevel` 如何限制后端、以及 `ActionLifecycle` 的 schema 校验。  
**适合谁读**：需要新增动作、调整策略阈值或排查“为什么这个动作被拒绝”的人。

## TL;DR

`PolicyEngine` 不构造 `AuthorizedAction`，只对每个 `(intent, action)` 输出 `Approved` 或 `Denied(DenialReason)`。
`ActionLifecycle` 再做 schema 校验、seal、执行和审计。

## PolicyEngine 配置默认值

| 字段 | 默认值 | 含义 |
| --- | --- | --- |
| `max_auto_risk` | `RiskLevel::Low` | 自动执行允许的最高风险 |
| `min_confidence` | `0.3` | 意图置信度下限 |
| `max_actions_per_batch` | `5` | 每个 intent 最多允许的动作数 |
| `blocked_actions` | `[]` | 按动作类型名子串匹配的黑名单 |

## 审查顺序

对每个 intent：

1. 后端能力风险检查：`capability.allows_risk(risk_level)`
2. 引擎全局风险检查：`risk_level > max_auto_risk`
3. 置信度检查：`intent.confidence < min_confidence`

对每个 action：

4. 每 intent 动作数上限
5. `blocked_actions` 子串匹配
6. `ActionUrgency::Deferred` 拒绝
7. 后端动作白名单：`capability.allows_action(action_type)`
8. target 校验（需要 `StructuredContext` 时）

## DenialReason 映射

| 触发条件 | `DenialReason` |
| --- | --- |
| 风险超过后端 `max_risk` | `RiskExceedsCapability` |
| 风险超过引擎配置 | `RiskExceedsConfig` |
| 置信度低于 `min_confidence` | `ConfidenceTooLow` |
| 动作数超过上限 | `BatchActionCapExceeded` |
| 命中 `blocked_actions` | `ActionTypeBlocked` |
| `urgency == Deferred` | `ActionUrgencyDeferred` |
| 动作不在后端白名单 | `ActionCapabilityDenied` |
| target 校验失败 | `TargetNotInContext` |

## CapabilityLevel 按路由

| 路由 | `max_risk` | 允许动作 |
| --- | --- | --- |
| `RuleBased` | `Low` | `NoOp`, `ReleaseMemory`, `KeepAlive` |
| `LocalEvaluator` | `Low` | 以上 + `PreWarmProcess`, `PrefetchFile` |
| `CloudLlm` | `Medium` | 全部动作 |
| `FallbackNoOp` | `Low` | `NoOp` |

## Target 校验规则

- `NoOp`：target 忽略，总是允许。
- `PreWarmProcess`：必须非空。
  - `own:*`、`notif:*` 无条件允许。
  - `pkg:<package>` 需在 `KnownTargets.packages` 中。
- `KeepAlive` / `ReleaseMemory` / `PrefetchFile`：
  - `None` 允许。
  - `Some("")` 拒绝。
  - 特殊前缀：`work:*`、`cache:prefetch`、`cache:all`、`url:*`、`uri:*`。
  - `pkg:<package>` 需存在已知包列表中。

`KnownTargets` 来自当前窗口 `StructuredContext`：
`foreground_apps`、`notified_apps`、`app_package`、`SanitizedEventType::AppTransition.package_name`、`SanitizedEventType::Notification.source_package` 等。

## ActionLifecycle schema 校验

在 policy 之前还有两道 schema 校验：

1. `PreWarmProcess` 必须有非空 target。
2. `EffectClass::PureRead` 的 action 不能承载 `RiskLevel::High` 的 intent。

校验失败直接产生 `RejectedInvalidSchema`，不进入 policy。

## 终态映射

```text
Succeeded              -> adapter 返回 Ok
RejectedInvalidSchema  -> schema 校验失败
DeniedByCapability     -> RiskExceedsCapability / ActionCapabilityDenied
DeniedByPolicy         -> 其他 policy 拒绝
Failed                 -> adapter 返回 Err
```

## 调试策略拒绝

在 `dipecsd` trace 中查看每个 `AuditRecord` 的 `denial_reason`。
常见排查：

- `RiskExceedsCapability`：后端能力不足，检查路由选择。
- `TargetNotInContext`：target 不在当前窗口已知包/文件列表中。
- `ConfidenceTooLow`：意图置信度低于 `min_confidence`。

## 相关文档

- [动作治理](action-governance.md)
- [状态机](states.md)
- [决策路由](decision-routing.md)
- [模型记忆与行为画像](model-memory.md)
