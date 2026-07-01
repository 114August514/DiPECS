# 代码地图

> Status: Current  
> Last verified: 2026-07-01

本文是源码导览。历史交付、slides 和 release notes 中的模块名可能已经变化，以本页为准。

## Rust Workspace

```text
crates/
  aios-spec/       # 协议和跨层数据结构
  aios-collector/  # Android JSONL + daemon/system source ingress
  aios-core/       # privacy / context / policy / lifecycle
  aios-agent/      # decision router and backends
  aios-action/     # action adapters
  aios-daemon/     # dipecsd runtime
  aios-cli/        # replay / audit / socket tooling
```

依赖方向：

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

aios-cli 复用 collector/core/agent/action 做离线 replay
```

`aios-action` 额外依赖 `aios-core`（为了 `ActionAdapter` trait 和 `AuthorizedAction` 类型），其余 library-layer crates 只依赖 `aios-spec`。

## `aios-spec`

| 文件 | 职责 |
| --- | --- |
| `event.rs` | `RawEvent`、`CollectorEnvelope`、`IngestedRawEvent`、source tier 和原始事件子类型。 |
| `sanitized.rs` | `SanitizedEvent` 和脱敏后的事件枚举。 |
| `context.rs` | `StructuredContext`、`ContextSummary`。 |
| `intent.rs` | `IntentBatch`、`Intent`、`SuggestedAction`、`CapabilityLevel`、`DenialReason`。 |
| `governance.rs` | `ActionProposal`、`ActionState`、`AuditRecord`、`ActionOutcome`、`PolicyActionDecision`。 |
| `trace.rs` | Golden trace / replay validation 数据结构。 |
| `bridge.rs` | Android localhost bridge 线协议（BridgeExecuteRequest、BridgeAuth、BridgeStatus 等），零依赖。 |

`aios-spec` 不应包含业务逻辑、平台 API 或运行时状态。

## `aios-collector`

| 文件 | 职责 |
| --- | --- |
| `android_jsonl.rs` | 解析 Android `CollectorEvent` JSONL，tail append-only `actions.jsonl`。 |
| `proc_reader.rs` | 扫描 `/proc`，生成进程资源事件。 |
| `system_collector.rs` | 系统状态快照。 |
| `binder_probe.rs` | Binder/eBPF 预留接口；当前为 stub，不产生真实事件。 |
| `fanotify_monitor.rs` | Fanotify 文件系统监控接口；接口存在，当前未接入 daemon loop。 |
| `collection_stats.rs` | 按 raw event kind 统计窗口内采集数量。 |

## `aios-core`

| 文件 | 职责 |
| --- | --- |
| `collector_ingress.rs` | 校验 external envelope，给内部 source 打 `SourceTier::Daemon`。 |
| `privacy_airgap.rs` | `RawEvent -> SanitizedEvent`，隐私边界。 |
| `context_builder.rs` | `WindowAggregator`，`SanitizedEvent -> StructuredContext`。 |
| `policy_engine.rs` | 逐 action 策略裁决，不构造 `AuthorizedAction`。 |
| `action_lifecycle.rs` | 唯一授权状态机，生成 `AuthorizedAction` 和 `AuditRecord`。 |
| `governance/mod.rs` | 私有字段 `AuthorizedAction` 和 `ActionAdapter` trait。 |
| `context_memory.rs` | `ModelMemoryStore`、`ProfileSummarizer`、`FeedbackEngine` — 从脱敏窗口和审计记录构建隐私保护模型记忆。 |
| `text_analysis.rs` | 文本和文件路径分析（ScriptHint、SemanticHint、ExtensionCategory），不保留原文。 |
| `action_bus.rs` | raw event / intent mpsc 通道封装。 |
| `trace_engine.rs` | Golden trace 验证。 |

## `aios-agent`

| 文件 | 职责 |
| --- | --- |
| `router.rs` | `DecisionRouter`、routing reason、circuit breaker、privacy sensitivity fallback。 |
| `backends/rule_based.rs` | 当前默认本地规则后端。 |
| `backends/local_evaluator.rs` | 无云 LLM 时的中高复杂度 fallback 评估后端。 |
| `backends/fallback.rs` | 熔断后的 `Idle + NoOp` 安全后端。 |
| `backends/cloud_llm/*` | 可选云端 LLM 后端、provider config、HTTP client、模型输出翻译。 |
| `backends/prefetch_target.rs` | cloud output 到 Android prefetch target 的保守映射。 |

## `aios-action`

| 文件 | 职责 |
| --- | --- |
| `lib.rs` | `DefaultActionExecutor`，纯确定性 stub，不访问网络或环境变量。 |
| `android_adapter.rs` | `AndroidAdapter`，经 localhost socket 转发 Android-safe action（HMAC、TCP 请求/响应协议）。 |
| `offline_adapter.rs` | replay / golden 使用的 deterministic adapter。 |

`DefaultActionExecutor` 只接收 `AuthorizedAction`。它不能自行 seal action。

## `aios-daemon`

| 文件 | 职责 |
| --- | --- |
| `main.rs` | `dipecsd` 二进制入口。 |
| `lib.rs` | runtime 装配、collection task、processing loop、CLI/env 参数解析。 |
| `pipeline.rs` | `process_window` 和 runtime trace recorder。 |
| `daemon.rs` | Linux daemonize 和 signal handling。 |

## `aios-cli`

| 文件 | 职责 |
| --- | --- |
| `main.rs` | `replay` 和 `send-authorized-action` CLI。当前 socket 命令只做 ping。 |
| `replay.rs` | JSONL replay、stage output、canonical audit hash。 |
| `android_bridge.rs` | Android socket ping/health-check。 |

## Android App

| 路径 | 职责 |
| --- | --- |
| `storage/EventStore.kt` | append-only `actions.jsonl` 写入、导出、清理。 |
| `model/AndroidRawEventMapper.kt` | Kotlin 事件到 Rust `RawEvent` JSON shape 的映射。 |
| `collectors/UsageCollector.kt` | `UsageStatsManager` 采集。 |
| `collectors/DeviceContextCollector.kt` | 设备上下文采集（电量、网络、屏幕等）。 |
| `services/NotificationCollectorService.kt` | 通知到达/移除采集。 |
| `services/CollectorForegroundService.kt` | heartbeat、collector lifecycle、manual action service entry。 |
| `services/AccessibilityCollectorService.kt` | AccessibilityService 采集入口（screening source）。 |
| `services/ActionMaintenanceJobService.kt` | `KeepAlive(work:*)` 的 JobService 实现。 |
| `services/BootReceiver.kt` | 开机自启 collector。 |
| `actions/AuthorizedActionSocketServer.kt` | localhost action socket、token、TTL、HMAC、rate limit。 |
| `actions/ActionExecutorBridge.kt` | Android-side action dispatch（含 `handleExecuteEnvelope` / `BridgeExecuteProtocol`）。 |
| `actions/AccessibleContentPrefetcher.kt` | `url:https://` / `uri:content://` prefetch。 |
| `actions/ActionMaintenanceScheduler.kt` | `KeepAlive(work:*)` 的 JobScheduler 实现。 |
| `actions/CacheTrimmer.kt` | `ReleaseMemory(cache:*)` 的 app-owned cache 清理。 |
| `actions/OwnResourceWarmer.kt` | `PreWarmProcess(own:*)` 的自身资源预热。 |
| `actions/SystemActionExecutors.kt` | 系统级 action 执行器（预装 app 预热等）。 |
| `actions/SystemPrewarmActivity.kt` | 系统预装 app 预热 activity。 |
| `actions/UserVisibleActionNotifier.kt` | 用户可见动作提示（非静默执行的安全确认）。 |

## 测试脚本 (tests/scenarios)

| 路径 | 职责 |
| --- | --- |
| `action-loop-e2e.sh` | 动作回路模拟器 end-to-end 验证。 |
| `emulator-e2e.sh` | Android 模拟器端到端采集验证。 |
| `lib/action-loop-stages.sh` | 动作回路分阶段验证逻辑。 |
| `lib/action-loop-selftest.sh` | 动作回路 selftest。 |
| `lib/emulator-e2e-stages.sh` | 模拟器分阶段验证逻辑。 |
| `lib/emulator-e2e-selftest.sh` | 模拟器 selftest。 |
| `lib/action-forensic-sender.py` | 动作取证消息发送器（mock socket）。 |

## 数据与实验

| 路径 | 职责 |
| --- | --- |
| `data/traces/` | replay / golden fixture。 |
| `data/evaluation/` | 动作回路和模拟器 e2e 评估结果（.md / .ndjson 报告）。 |
| `data/schemas/` | CollectorEnvelope / RawEvent 等 JSON schema。 |
| `lab4/` | 课程 Lab4 llama.cpp、RPC、Ray 实验；不是 DiPECS runtime 主链路。 |
| `docs/src/` | MkDocs 文档。 |
| `docs/academic-src/` | LaTeX 学术报告源。 |
