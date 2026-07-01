# 数据流

> Status: Current  
> Last verified: 2026-06-30

当前仓库有三条输入路径：真实 Android 采集、daemon 本机采集、离线 replay。它们在 Rust 侧都会收敛到 `IngestedRawEvent -> PrivacyAirGap -> WindowAggregator`。

## 1. Android 采集路径

Android app 是当前最重要的真实数据源。它使用公开 Android API 捕获信号，写入 app 私有目录下的 append-only JSONL：

```text
<app-private-files>/traces/actions.jsonl
```

每行是一个 `CollectorEvent`。只有 `rawEvent` 非空且符合 `aios-spec::RawEvent` 外部标签格式的行会进入生产管线。

已 promoted 的 Android 来源：

| Android 来源 | Kotlin 入口 | Rust RawEvent |
| --- | --- | --- |
| `UsageStatsManager` | `UsageCollector` | `AppTransition` / `ScreenState` |
| `NotificationListenerService` | `NotificationCollectorService` | `NotificationPosted` / `NotificationInteraction` |
| device context heartbeat | `CollectorForegroundService` | `SystemState` |

`AccessibilityService` 目前仍是 screening source。它可以写 JSONL 供界面预览和调研，但没有 Rust schema 时会写 `rawEvent: null`，Rust ingress 会跳过。

Rust 入口是 `aios_collector::android_jsonl::AndroidJsonlTailer`。它保留文件 offset，只读取新增完整行；文件截断或轮转时会重置 offset。

## 2. Daemon 本机采集路径

`dipecsd` 的采集 task 会周期性生成内部事件：

| 来源 | 当前状态 | RawEvent |
| --- | --- | --- |
| `/proc` 差分 | 已接入 | `ProcStateChange` |
| system snapshot | 已接入 | `SystemState` |
| BinderProbe | 接口存在，当前 stub | `BinderTransaction` 预留 |
| fanotify / VFS | spec 预留，未接入采集器 | `FileSystemAccess` 预留 |

内部采集通过 `RustCollectorIngress::accept_internal` 进入管线，并标为 `SourceTier::Daemon`。Android JSONL 入口则标为 `SourceTier::PublicApi`。

## 3. Replay 路径

`aios-cli replay` 读取 Android `CollectorEvent` JSONL 形状的文件，例如 `data/traces/sample_replay.jsonl`。它提取每行 `rawEvent`，合成 `CollectorEnvelope`，再进入同一套 Rust 管线。

Replay 和 daemon 的主要差异：

| 项 | daemon | replay |
| --- | --- | --- |
| 时间来源 | wall-clock + timer | trace timestamp |
| 执行器 | `AndroidAdapter`（若启用 bridge）或 `DefaultActionExecutor` | `OfflineAdapter` |
| 输出 | tracing + optional runtime NDJSON | stage NDJSON + optional canonical audit |
| 目标 | 在线运行 | 回归、golden、隐私泄漏检测 |

## 隐私边界

`RawEvent` 只允许存在于 collector 到 core 的短路径内。经过 `DefaultPrivacyAirGap` 后，系统只传递 `SanitizedEvent` 和 `StructuredContext`。

当前脱敏要点：

- 通知标题和正文变成 `TextHint` 与 `SemanticHint`。
- 文件路径只用于扩展名类别分类，不保留原路径。
- Binder payload 不保存。
- 通知 interaction 的 Android notification key 被丢弃，避免 tag 携带 PII。

## 上下文窗口

`WindowAggregator` 默认按 10 秒窗口聚合 `SanitizedEvent`，生成 `StructuredContext`。摘要字段包括：

- `foreground_apps`
- `notified_apps`
- `all_semantic_hints`
- `file_activity`
- `latest_system_status`
- `source_tier`

`StructuredContext` 是决策后端可见的唯一上下文格式。
