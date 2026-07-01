# Replay 与审计

> Status: Current  
> Last verified: 2026-06-30

`aios-cli replay` 是当前验证核心管线的主要工具。它读取 Android `CollectorEvent` JSONL，复用生产管线，但用 deterministic execution 替换真实动作执行。

## 基本命令

```bash
cargo run -p aios-cli -- replay data/traces/sample_replay.jsonl \
  --stages policy \
  --audit data/evaluation/audit.ndjson
```

可选 stage：

| Stage | 包含 |
| --- | --- |
| `ingest` | parse `rawEvent`，进入 `RustCollectorIngress`。 |
| `sanitize` | 执行 `DefaultPrivacyAirGap`。 |
| `context` | 关闭窗口并输出 `StructuredContext` 摘要。 |
| `decision` | 执行 `DecisionRouter`。 |
| `policy` | 只做策略裁决，不执行 adapter。 |
| `execute` | 运行完整 `ActionLifecycle` + `OfflineAdapter`。 |

## Determinism

Replay 的窗口由 trace timestamp 驱动，不使用 wall-clock。执行阶段注入 `OfflineAdapter`，不访问 Android、网络或真实系统。

每条 stage 记录会同时写入：

- human-facing output sink
- canonical audit sink
- SHA-256 hasher

canonical projection 会排序对象 key，并剥离 volatile 字段，例如 UUID、`window_id`、`intent_id`、`latency_us`。最终 summary 中的 `audit_hash` 用于 golden regression。

## 当前 fixtures

| 文件 | 用途 |
| --- | --- |
| `data/traces/sample_replay.jsonl` | 主 replay / audit hash fixture。 |
| `data/traces/denial.jsonl` | policy/capability denial fixture。 |
| `data/traces/noop_matrix.jsonl` | NoOp / routing / event mix fixture。 |
| `data/traces/golden_sample.json` | `TraceEngine` golden sample。 |

## 与 daemon 的差异

| 项 | daemon | replay |
| --- | --- | --- |
| 输入 | live channels + tailer | JSONL reader |
| 窗口 | timer | captured timestamp |
| adapter | `DefaultActionExecutor` | `OfflineAdapter` |
| 输出 | tracing / runtime NDJSON | NDJSON / canonical audit / hash |
| 目标 | 在线原型 | 回归和证据链 |
