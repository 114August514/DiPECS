# 管线与运行时

> Status: Current  
> Last verified: 2026-07-01  
> Runtime entry: `crates/aios-daemon/src/lib.rs`

`dipecsd` 是 DiPECS 在线管线的宿主。它负责把采集、脱敏、聚合、决策、策略、
动作治理和 runtime trace 组装成在线管线，不定义协议，不直接实现 Android API，
也不绕过 `aios-core` 的授权状态机。

入口是 `crates/aios-daemon/src/main.rs`，实际逻辑在 `crates/aios-daemon/src/lib.rs`。

## 启动方式

前台开发模式：

```bash
RUST_LOG=info cargo run -p aios-daemon --bin dipecsd -- --no-daemon
```

接入 Android JSONL：

```bash
RUST_LOG=info cargo run -p aios-daemon --bin dipecsd -- \
  --no-daemon \
  --android-trace-jsonl path/to/actions.jsonl \
  --trace-output data/evaluation/runtime.ndjson
```

等价环境变量：

```bash
DIPECS_ANDROID_TRACE_JSONL=path/to/actions.jsonl
DIPECS_RUNTIME_TRACE_OUTPUT=data/evaluation/runtime.ndjson
```

## 运行图

```text
                         optional cloud HTTPS
                               │
                               ▼
                    aios-agent::DecisionRouter
                               │
                               ▼
┌──────────────────────────────────────────────────────────────┐
│                         dipecsd                              │
│                                                              │
│  Task 1: collection                                          │
│    /proc diff                                                │
│    system snapshot                                           │
│    BinderProbe poll (stub)                                   │
│    AndroidJsonlTailer poll                                   │
│        │                                                     │
│        ▼                                                     │
│    ActionBus.raw_events_tx                                   │
│                                                              │
│  Task 2: processing                                          │
│    ActionBus.raw_events_rx                                   │
│        -> PrivacyAirGap                                      │
│        -> WindowAggregator                                   │
│        -> ModelMemoryStore.model_input(ctx)                  │
│        -> DecisionRouter.evaluate_model_input                │
│        -> ActionLifecycle                                    │
│        -> ModelMemoryStore.update()                          │
│        -> ProfileSummarizer                                  │
│        -> RuntimeTraceRecorder                               │
└──────────────────────────────────────────────────────────────┘
```

## Collection task

当前 collection task 包含四类入口：

| 入口 | 当前实现 | SourceTier |
| --- | --- | --- |
| `/proc` | `ProcReader::scan_all` + diff | `Daemon` |
| system snapshot | `SystemStateCollector::snapshot` | `Daemon` |
| BinderProbe | 初始化检测 + `poll()`；当前无真实事件 | `Daemon` |
| Android JSONL | `AndroidJsonlTailer::poll()` | `PublicApi` |

Android JSONL 只有在传入 `--android-trace-jsonl` 或设置 `DIPECS_ANDROID_TRACE_JSONL` 时启用。

processing loop 等待三类事件：

1. shutdown signal
2. `bus.recv_raw()`
3. window deadline

收到 raw event 时：

```text
RawEvent + SourceTier
  -> sanitizer.sanitize_with_tier(...)
  -> window.push(SanitizedEvent)
```

## 窗口处理

窗口关闭后执行：

```text
WindowAggregator.close(...)
  -> StructuredContext
  -> ModelMemoryStore.model_input(ctx)
  -> DecisionRouter.evaluate_model_input(input)
  -> CapabilityLevel::for_route(route)
  -> ActionLifecycle.run(window_ordinal, batch, route, backend_error, capability, ctx)
  -> ModelMemoryStore.update(...)
```

`process_window` 会统计：

- 关闭窗口的事件数
- 原始事件计数
- decision route / model / latency / error
- action audit records
- executed / denied / failed 数量
- model memory 更新（行为画像轮转、近期决策记录、反馈推导）

## 默认周期

| 项 | 周期 |
| --- | --- |
| Binder poll loop sleep | 100 ms |
| Android JSONL poll | 500 ms |
| system snapshot | 30 s |
| context window | 10 s |

## Runtime trace

`--trace-output <path>` 会追加 NDJSON，每个窗口一行，包含：

- window metadata
- raw event stats
- context summary
- decision route / model / rationale / error
- audit records

## 当前限制

- `BinderProbe` 当前没有真实 eBPF 程序加载和 ring/perf buffer 读取逻辑；`poll()` 返回空。
- fanotify / 文件系统采集器未接入 daemon loop。
- daemon 直接运行在普通 Linux 上时，只能读取宿主 `/proc` 和系统 fallback 信息。
- Android JSONL 不是自动发现路径，必须通过参数或环境变量提供。
- daemon 内部不会直接生成 `AuthorizedAction`；这必须经过 `ActionLifecycle`。

## 相关文档

- [数据流](data-flow.md)
- [动作治理](action-governance.md)
- [验证与审计](verification.md)
