# Daemon 运行时

> Status: Current  
> Last verified: 2026-06-30

`dipecsd` 是当前 Rust 在线管线的宿主，入口是 `crates/aios-daemon/src/main.rs`，实际逻辑在 `crates/aios-daemon/src/lib.rs`。

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

## 两个 task

```text
Task 1: collection
  /proc diff
  system snapshot
  BinderProbe poll (currently no real events)
  AndroidJsonlTailer poll
      -> ActionBus.raw_events_tx

Task 2: processing
  ActionBus.raw_events_rx
      -> PrivacyAirGap
      -> WindowAggregator
      -> process_window
      -> DecisionRouter
      -> ActionLifecycle
      -> RuntimeTraceRecorder
```

默认周期：

| 项 | 周期 |
| --- | --- |
| Binder poll loop sleep | 100 ms |
| Android JSONL poll | 500 ms |
| system snapshot | 30 s |
| context window | 10 s |

## 窗口处理

窗口关闭后执行：

```text
DecisionRouter.evaluate(ctx)
CapabilityLevel::for_route(route)
ActionLifecycle.run(window_ordinal, batch, route, backend_error, capability, ctx)
```

`process_window` 会统计：

- 关闭窗口的事件数
- 原始事件计数
- decision route / model / latency / error
- action audit records
- executed / denied / failed 数量

如果设置 `--trace-output`，daemon 会以 NDJSON 追加写一条 `daemon_window` 记录。

## 当前限制

- `BinderProbe` 当前没有真实 eBPF 程序加载和 ring/perf buffer 读取逻辑；`poll()` 返回空。
- fanotify / 文件系统采集器未接入 daemon loop。
- daemon 直接运行在普通 Linux 上时，只能读取宿主 `/proc` 和系统 fallback 信息。
- Android JSONL 不是自动发现路径，必须通过参数或环境变量提供。
