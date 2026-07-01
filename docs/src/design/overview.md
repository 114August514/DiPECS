# 架构概览

> Status: Current summary  
> Last verified: 2026-07-01  
> 具体运行细节见 [当前实现](../current/overview.md)。

DiPECS 采用机制-策略分离。机制层负责采集、脱敏、聚合、审计和受控执行；策略层只能基于脱敏上下文生成意图和建议动作。

## 当前分层

```text
aios-spec
  ├─ aios-collector
  ├─ aios-core
  ├─ aios-agent
  └─ aios-action (also depends on aios-core)

aios-collector ─┐
aios-core ──────┼─ aios-daemon
aios-agent ─────┤
aios-action ────┘

aios-cli 复用核心管线做 replay / audit
apps/android-collector 通过 JSONL 与 action socket 对接 Rust 管线
```

## 当前数据流

```text
apps/android-collector / daemon sources / replay JSONL
    -> CollectorEnvelope / IngestedRawEvent
    -> RawEvent
    -> PrivacyAirGap
    -> SanitizedEvent
    -> WindowAggregator
    -> StructuredContext
    -> DecisionRouter
    -> IntentBatch
    -> PolicyEngine + CapabilityLevel
    -> ActionLifecycle
    -> AuthorizedAction
    -> ActionAdapter
    -> AuditRecord / runtime trace / replay audit_hash
```

## 核心边界

| 边界 | 当前规则 |
| --- | --- |
| Android -> Rust | 生产入口是 append-only `actions.jsonl`；`rawEvent: null` 被跳过。 |
| PrivacyAirGap | `RawEvent` 不越过此边界；模型后端只接收 `StructuredContext`。 |
| Decision | 后端只输出 `IntentBatch`，不能构造可执行动作。 |
| Policy | `PolicyEngine` 只产出逐 action 裁决。 |
| Authorization | `ActionLifecycle` 是 `AuthorizedAction` 唯一构造点。 |
| Execution | `DefaultActionExecutor` / `OfflineAdapter` 都实现 `ActionAdapter`。 |
| Audit | 每个 `ActionCoord` 恰好产出一条终态 `AuditRecord`。 |

## 验证与回归

| 机制 | 说明 |
| --- | --- |
| Golden Trace | `aios-core::trace_engine` 基于 fixture 做确定性 replay 验证。 |
| Action-Loop 模拟器 | `tests/scenarios/action-loop-e2e.sh` 通过 mock-socket 做动作回路端到端验证。 |
| Emulator E2E | `tests/scenarios/emulator-e2e.sh` 在 Android 模拟器上验证采集链路。 |

## 已实现与预留

| 主题 | 当前事实 |
| --- | --- |
| Android public API source | 已接入：UsageStats、NotificationListener、DeviceContext。 |
| AccessibilityService | 只作为 screening source；未进入生产 Rust schema。 |
| Cloud LLM | 可选，默认关闭；配置错误或失败会回落到本地规则。 |
| Binder/eBPF | 接口存在但真实 eBPF 未实现；`poll()` 当前无事件。 |
| fanotify / system image | spec 和文档预留，非主线实现。 |
| Android actions | 已有安全子集：prefetch、cache trim、maintenance job、自身资源 warmup、系统预装预热、用户可见提示。 |
| Action-Loop 验证 | mock-socket e2e 测试 + 模拟器分阶段验证脚本已实现。 |

## 文档阅读建议

- 当前运行链路：从 [当前实现总览](../current/overview.md) 开始。
- 模块职责：看 [代码地图](crates-map.md)。
- 动作治理：看 [动作治理](../current/action-governance.md) 和 [RFC-0002](rfc/0002-action-bus-governance.md)。
- 历史设计：`releases/`、`slides/`、`team/meetings/` 反映当时状态，不能直接当当前实现。
