# CLI 参考

> Status: Current  
003e Last verified: 2026-07-01  
> Code anchors: `crates/aios-cli/src/main.rs`, `crates/aios-cli/src/replay.rs`, `crates/aios-cli/src/android_bridge.rs`

**这篇文档回答什么**：`aios-cli` 各子命令的用法、参数和输出格式。  
**适合谁读**：需要 replay trace、验证 action socket 或派发测试动作的人。

## TL;DR

`aios-cli` 有三个子命令：

- `replay`：离线回放 JSONL trace，输出 stage NDJSON 和 canonical audit。
- `send-authorized-action`：ping Android action socket，验证 token 和连通性。
- `send-action`：派发一个真实的 HMAC-signed 动作到 Android action socket。

## `aios-cli replay`

```bash
aios-cli replay <PATH> \
  [--window-secs <SECONDS>] \
  [--stages <STAGE>] \
  [--output <PATH>] \
  [--audit <PATH>]
```

参数：

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `PATH` | — | JSONL trace 文件 |
| `--window-secs` | `10` | 窗口聚合秒数 |
| `--stages` | `Policy` | 最高运行阶段 |
| `--output` | stdout | NDJSON 输出 |
| `--audit` | — | canonical audit 日志 |

阶段：

| Stage | 说明 |
| --- | --- |
| `Ingest` | 接受 `CollectorEnvelope` |
| `Sanitize` | `PrivacyAirGap` 脱敏 |
| `Context` | 窗口聚合 |
| `Decision` | `DecisionRouter` 决策 |
| `Policy` | `PolicyEngine` 审查（不执行） |
| `Execute` | `ActionLifecycle` + `OfflineAdapter` 完整执行 |

示例：

```bash
cargo run -p aios-cli -- replay data/traces/sample_replay.jsonl \
  --stages execute \
  --audit data/evaluation/audit.ndjson
```

### Audit hash

`replay` 对每个 per-stage audit 记录计算 SHA-256 `audit_hash`：

- key 排序
- 剥离 volatile key：`event_id`、`window_id`、`intent_id`、`latency_us`、`backend_error`

摘要行包含 `audit_hash`，但不参与 hash 计算。

## `aios-cli send-authorized-action`

Ping / health-check：

```bash
cargo run -p aios-cli -- send-authorized-action \
  --host 127.0.0.1 \
  --port 46321 \
  --auth-token <token>
```

只发送 ping，不派发动作。

## `aios-cli send-action`

真实动作派发（注意：会真实执行）：

```bash
cargo run -p aios-cli -- send-action \
  --host 127.0.0.1 \
  --port 46321 \
  --auth-token <token> \
  --action-type KeepAlive \
  --target work:collector_heartbeat \
  --urgency IdleTime
```

参数：

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `--action-type` | `NoOp` | `NoOp` / `PrefetchFile` / `KeepAlive` / `ReleaseMemory` / `PreWarmProcess` |
| `--target` | `""` | action target |
| `--urgency` | `Immediate` | `Immediate` / `IdleTime` / `Deferred` |

HMAC 覆盖：

```text
dipecs.android.action.v1
issued_at_ms:<issued>
expires_at_ms:<expires>
action_type:<len>:<type>
target:<len>:<target>
urgency:<len>:<urgency>
```

## 相关文档

- [验证与审计](../architecture/verification.md)
- [动作执行](../architecture/action-execution.md)
- [Android 动作实现手册](../android/action-bridge.md)
- [调试指南](../team/debugging.md)
