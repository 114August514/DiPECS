# 当前实现总览

> Status: Current  
> Last verified: 2026-06-30  
> Scope: 仓库当前可运行的 DiPECS 主链路，不包含早期设想和课程实验目录。

DiPECS 当前是一个 Android/Linux AIOS 原型。它把本地信号采集、隐私脱敏、上下文聚合、决策路由、策略审查和授权动作执行拆成独立边界，避免原始 Android 事件直接进入模型或动作执行器。

当前默认运行路径不是“云端 LLM 主导”。`DecisionRouter` 默认优先使用本地 `RuleBasedBackend`；当窗口带有本地可行动信号或云端未启用/不可用时，会路由到 `LocalEvaluatorBackend`；`CloudLlmBackend` 只有在环境变量启用且配置完整时才参与路由。云端失败会回落到本地规则，连续错误触发熔断后进入 `FallbackNoOpBackend`。

## 当前主链路

```text
Android collector JSONL / daemon system sources / replay fixture
    -> aios-collector / RustCollectorIngress
    -> IngestedRawEvent
    -> aios-core::PrivacyAirGap
    -> SanitizedEvent
    -> WindowAggregator
    -> StructuredContext
    -> aios-agent::DecisionRouter
    -> IntentBatch
    -> PolicyEngine + CapabilityLevel
    -> ActionLifecycle
    -> AuthorizedAction
    -> ActionAdapter
    -> AuditRecord / runtime trace / replay audit hash
```

## 已实现能力

| 区域 | 当前状态 |
| --- | --- |
| Android 采集 | `apps/android-collector` 写 append-only `actions.jsonl`；非空 `rawEvent` 行进入 Rust 管线。 |
| Android promoted sources | `UsageStatsManager`、`NotificationListenerService`、`DeviceContext`。 |
| Android screening sources | `AccessibilityService` 只记录和预览；`rawEvent: null` 行被 Rust 跳过。 |
| Daemon 采集 | `/proc` 进程差分、系统状态快照、Android JSONL tail。 |
| Binder/eBPF | 只有 `BinderProbe` 接口和检测 stub；当前不产生真实 Binder 事件。 |
| 隐私边界 | `RawEvent` 只在 collector-core 边界内存在；模型后端只接收 `StructuredContext`。 |
| 决策 | `RuleBasedBackend`、`LocalEvaluatorBackend`、可选 `CloudLlmBackend`、`FallbackNoOpBackend`。 |
| 策略 | `PolicyEngine` 做风险、置信度、能力、urgency、target-in-context 检查。 |
| 动作治理 | `ActionLifecycle` 是 `AuthorizedAction` 唯一构造点，每个建议动作产出一条终态 `AuditRecord`。 |
| 动作执行 | `DefaultActionExecutor` 默认本地 stub；启用 Android bridge 后转发 Android-safe target。 |
| 离线验证 | `aios-cli replay` 使用 `OfflineAdapter`，生成 canonical audit stream 和 `audit_hash`。 |

## 非主线或历史内容

- `lab4/` 是 llama.cpp、RPC、Ray 推理实验，不属于 DiPECS runtime 主链路。
- `docs/src/slides/`、`docs/src/team/meetings/`、`docs/src/research/deliverables/` 是历史交付和答辩材料，可能保留当时表述。
- `docs/src/design/releases/v0.2.md` 是历史 release note，不应作为当前实现说明阅读。

## 阅读顺序

1. [数据流](data-flow.md)
2. [Daemon 运行时](runtime.md)
3. [动作治理](action-governance.md)
4. [Android 桥接](android-bridge.md)
5. [Replay 与审计](replay-audit.md)
