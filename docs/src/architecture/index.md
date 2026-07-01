# 架构概览

> Status: Current summary  
> Last verified: 2026-07-01  
> Scope: 仓库当前可运行的 DiPECS 主链路，不包含早期设想和课程实验目录。

DiPECS 是一个 Android/Linux AIOS 原型。它把本地信号采集、隐私脱敏、上下文聚合、
决策路由、策略审查和授权动作执行拆成独立边界，避免原始 Android 事件直接进入模型或
动作执行器。

系统采用**机制-策略分离**：机制层负责采集、脱敏、聚合、审计和受控执行；
策略层只能基于脱敏上下文生成意图和建议动作。

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

## 主链路

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
    -> AuditRecord / runtime trace / replay audit_hash
```

当前默认运行路径不是“云端 LLM 主导”。`DecisionRouter` 默认优先使用本地
`RuleBasedBackend`；当窗口带有本地可行动信号或云端未启用/不可用时，会路由到
`LocalEvaluatorBackend`；`CloudLlmBackend` 只有在环境变量启用且配置完整时才参与路由。
云端失败会回落到本地规则，连续错误触发熔断后进入 `FallbackNoOpBackend`。

## 核心边界

| 边界 | 当前规则 |
| --- | --- |
| Android -> Rust | 生产入口是 append-only `actions.jsonl`；`rawEvent: null` 被跳过。 |
| PrivacyAirGap | `RawEvent` 不越过此边界；模型后端只接收 `StructuredContext`。 |
| Decision | 后端只输出 `IntentBatch`，不能构造可执行动作。 |
| Policy | `PolicyEngine` 只产出逐 action 裁决。 |
| Authorization | `ActionLifecycle` 是 `AuthorizedAction` 唯一构造点。 |
| Execution | `DefaultActionExecutor` / `OfflineAdapter` / `AndroidAdapter` 都实现 `ActionAdapter`。 |
| Audit | 每个 `ActionCoord` 恰好产出一条终态 `AuditRecord`。 |

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

## 非主线或历史内容

- `lab4/` 是 llama.cpp、RPC、Ray 推理实验，不属于 DiPECS runtime 主链路。
- `docs/src/slides/`、`docs/src/team/meetings/`、`docs/src/research/deliverables/` 是历史交付和答辩材料，可能保留当时表述。
- `rfc/releases/v0.2.md` 是历史 release note，不应作为当前实现说明阅读。

## 阅读顺序

1. [数据流](data-flow.md)
2. [管线与运行时](pipeline.md)
3. [动作治理](action-governance.md)
4. [动作执行与 Android bridge](action-execution.md)
5. [验证与审计](verification.md)
6. [代码地图](crates.md)
7. [状态机](states.md)
8. [设计哲学](philosophy.md)
