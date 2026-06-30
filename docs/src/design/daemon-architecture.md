# Daemon 架构

> Status: Current  
> Last verified: 2026-06-30  
> Runtime entry: `crates/aios-daemon/src/lib.rs`

`dipecsd` 负责把采集、脱敏、聚合、决策、策略、动作治理和 runtime trace 组装成在线管线。它不定义协议，不直接实现 Android API，也不绕过 core 的授权状态机。

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
│        -> process_window                                     │
│        -> DecisionRouter                                     │
│        -> ActionLifecycle                                    │
│        -> DefaultActionExecutor                              │
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

## Processing task

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

窗口到期或 flush 时：

```text
WindowAggregator.close(...)
  -> StructuredContext
  -> pipeline::process_window(...)
```

## Window processing

`process_window` 只处理一个已关闭窗口：

```text
router.evaluate(ctx)
capability = CapabilityLevel::for_route(route)
lifecycle.run(window_ordinal, batch, route, backend_error, capability, ctx)
```

返回的 `AuditRecord` 会用于：

- 统计 executed / denied / failed
- tracing log
- optional runtime NDJSON

## Runtime trace

`--trace-output <path>` 会追加 NDJSON，每个窗口一行，包含：

- window metadata
- raw event stats
- context summary
- decision route / model / rationale / error
- audit records

## 当前未实现或预留

- Binder/eBPF：`BinderProbe` 没有加载 BPF ELF，没有 attach tracepoint，没有读取 perf/ring buffer。
- fanotify：`RawEvent::FileSystemAccess` 存在，但 daemon 未接入采集器。
- JNI ingress：文档历史上提到过，但当前生产入口是 JSONL tail。
- daemon 内部不会直接生成 `AuthorizedAction`；这必须经过 `ActionLifecycle`。

## 相关文档

- [当前 Daemon 运行时](../current/runtime.md)
- [数据流](../current/data-flow.md)
- [动作治理](../current/action-governance.md)
