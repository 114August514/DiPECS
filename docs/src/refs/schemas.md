# Schema 参考

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `crates/aios-spec/src/`, `data/schemas/datapath.md`, `data/traces/`, `data/evaluation/`

**这篇文档回答什么**：DiPECS 里所有 JSON/NDJSON 数据的来源、格式、关键字段和对应的 Rust/Kotlin 类型。  
**适合谁读**：需要对接数据、写测试 fixture、调试 trace，或者给评估数据集做后处理的人。

## TL;DR

DiPECS 没有单独的 JSON Schema 文件；schema 的单一事实来源是 Rust `serde` 类型（`crates/aios-spec/src/`）和 Kotlin 采集器模型。数据产物分为四类：

1. **Android 采集器 JSONL**：每行一个 `CollectorEvent` 信封，含 `rawEvent`。
2. **内部流转类型**：`RawEvent` → `SanitizedEvent` → `StructuredContext` → `ModelInput` → `IntentBatch`。
3. **Runtime / Replay NDJSON**：`dipecsd` 和 `aios-cli replay` 按 stage 输出的审计流。
4. **评估数据集 JSON**：`resource_overhead`、`ux_metrics`、`stability`、`cloud_latency`、`cloud_scenarios`。

所有原始 PII 在写入 collector JSONL 前已被 redact；Rust 管线只接触脱敏后的数据。

## 数据产物总览

| 产物 | 格式 | 主要 Rust/Kotlin 类型 | 位置 |
| --- | --- | --- | --- |
| Android 采集器 trace | JSONL | `CollectorEvent` (Kotlin) / `CollectorEnvelope` (Rust) | `data/traces/*.jsonl` |
| Runtime trace | NDJSON | `RuntimeTraceRecord` | `data/evaluation/*.ndjson` |
| Replay audit | NDJSON | `GoldenTrace` / `ReplayResult` | `data/evaluation/*.audit` |
| 评估数据集 | JSON | 专用 schema（见下文） | `data/evaluation/*.json` |
| Golden fixture | JSON | `GoldenTrace` | `data/traces/golden_sample.json` |

## Android 采集器 JSONL

每行一个 `CollectorEvent`：

```json
{
  "eventId": "f9d4e449-b32d-4e24-b516-3c960c891a97",
  "timestampMs": 1782840757348,
  "source": "usage_stats",
  "eventType": "activity_stopped",
  "packageName": "com.dipecs.collector",
  "className": "com.dipecs.collector.MainActivity",
  "windowTitle": null,
  "text": null,
  "action": "activity_stopped",
  "deviceContext": {
    "timezone": "Asia/Shanghai",
    "batteryPercent": 100,
    "isCharging": true,
    "networkType": "wifi",
    "isScreenOn": true,
    "ringerMode": "normal",
    "doNotDisturbMode": 1,
    "locationType": "Unknown",
    "headphoneConnected": false,
    "bluetoothConnected": false
  },
  "rawEvent": {
    "AppTransition": {
      "timestamp_ms": 1782840757348,
      "package_name": "com.dipecs.collector",
      "activity_class": "com.dipecs.collector.MainActivity",
      "transition": "Background"
    }
  },
  "rawPayload": { "usageEventType": 23 }
}
```

要点：

- `rawEvent: null` 的行会被 Rust 跳过（通常是 accessibility screening source）。
- 敏感字段（`raw_title`、`raw_text`、`text`、`target`、`cachePath` 等）已在写入前 redact。
- Rust 侧会再包一层 `CollectorEnvelope`，`schema_version = "dipecs.collector.v1"`，`source_tier = "PublicApi"`。

## 内部类型速查

| 类型 | 文件 | 说明 |
| --- | --- | --- |
| `RawEvent` | `event.rs` | 原始 sensor 事件枚举 |
| `CollectorEnvelope` | `event.rs` | 外部入口信封 |
| `IngestedRawEvent` | `event.rs` | 带 `SourceTier` 的内部 bus 事件 |
| `SanitizedEvent` | `sanitized.rs` | 脱敏后事件 |
| `StructuredContext` | `context.rs` | 一个窗口的聚合上下文 |
| `ContextSummary` | `context.rs` | 聚合摘要 |
| `ModelInput` | `context.rs` | 后端输入 = 当前上下文 + 画像 + 反馈 |
| `UserBehaviorProfile` | `context.rs` | 滚动行为画像 |
| `RecentDecisionRecord` | `context.rs` | 单窗口决策反馈 |
| `IntentBatch` | `intent.rs` | 后端输出意图集合 |
| `DecisionBackendResult` | `intent.rs` | 统一后端输出 |
| `SuggestedAction` | `intent.rs` | 建议动作 |
| `ActionProposal` | `governance.rs` | 未授权动作信封 |
| `AuthorizedAction` | `aios-core/src/governance/mod.rs` | 已授权、不可伪造的执行凭证（仅 `Serialize`） |
| `AuditRecord` | `governance.rs` | 动作生命周期审计记录 |
| `BridgeExecuteRequest` / `BridgeExecuteResponse` | `bridge.rs` | Android bridge IPC 信封 |
| `GoldenTrace` / `ReplayResult` | `trace.rs` | Golden fixture 与 replay 结果 |

关键枚举：`SourceTier`、`AppTransition`、`SemanticHint`、`DecisionRoute`、`RiskLevel`、`ActionType`、`ActionUrgency`、`DenialReason`、`ActionState`、`FeedbackCorrectness`。

## Runtime NDJSON trace

`dipecsd` 在 `--trace-output` 下每窗口输出一行：

```json
{
  "stage": "daemon_window",
  "window_ordinal": 0,
  "window_id": "...",
  "window_start_ms": 1234567890000,
  "window_end_ms": 1234567900000,
  "duration_secs": 10,
  "event_count": 3,
  "raw_event_total": 5,
  "raw_event_stats": { ... },
  "context_summary": { ... },
  "behavior_profile": { ... },
  "decision": {
    "route": "CloudLlm",
    "model": "...",
    "intent_count": 2,
    "rationale_tags": [...],
    "latency_us": 4200,
    "error": null
  },
  "audit": [ ... ]
}
```

## Replay audit NDJSON

`aios-cli replay` 按 stage 输出：

```text
ingest
sanitize
context
decision
policy
execute
summary
```

每个 stage 除了 `summary` 都会进入 canonical audit stream，volatile key（`event_id`、`window_id`、`intent_id`、`latency_us`、`backend_error`）会被剥离后计算 `audit_hash`。

## 评估数据集 schema

### `dipecs.resource_overhead.v1`

`data/evaluation/resource-overhead-emulator-*.json`

| 字段 | 含义 |
| --- | --- |
| `schema_version` | 固定 `"dipecs.resource_overhead.v1"` |
| `dataset_id` | 数据集标识 |
| `environment` | 设备、ABI、DiPECS 版本等 |
| `runs[]` | 每种模式的多组样本 |
| `thresholds` | CPU / PSS / RSS 阈值 |
| `deltas_vs_baseline` | 相对 baseline 的增量 |
| `conclusion.accepted` | 是否通过阈值 |

### `dipecs.ux_metrics.v1`

`data/evaluation/ux-metrics-emulator-*.json`

| 字段 | 含义 |
| --- | --- |
| `schema_version` | 固定 `"dipecs.ux_metrics.v1"` |
| `runs[]` | `cold_startup`、`prewarm_startup`、`release_memory` 等模式 |
| `ux_deltas` | PreWarm 加速、ReleaseMemory jank 变化 |
| `thresholds` | 非回归阈值 |
| `conclusion.accepted` | 是否通过阈值 |

### `dipecs.stability.v1`

`data/evaluation/stability-emulator-*.json`

| 字段 | 含义 |
| --- | --- |
| `schema_version` | 固定 `"dipecs.stability.v1"` |
| `results.rss_growth_per_hour_mb` | RSS 每小时增长 |
| `results.pss_growth_per_hour_mb` | PSS 每小时增长 |
| `results.avg_cpu_pct` | 平均 CPU |
| `thresholds` | `max_rss_growth_per_hour_mb` 等 |
| `conclusion.accepted` | 是否通过阈值 |

### `dipecs.cloud_latency.v1`

`data/evaluation/cloud-latency-*.json`

| 字段 | 含义 |
| --- | --- |
| `schema_version` | 固定 `"dipecs.cloud_latency.v1"` |
| `environment.model` / `provider` | 模型与 provider |
| `results.latency_ms` | min/p50/p95/max |
| `results.success_rate` | 成功率 |
| `thresholds` | 延迟阈值 |
| `conclusion.accepted` | 是否通过阈值 |

### `dipecs.cloud_scenarios.v1`

`data/evaluation/cloud-scenarios-*.json`

| 字段 | 含义 |
| --- | --- |
| `schema_version` | 固定 `"dipecs.cloud_scenarios.v1"` |
| `environment.scenarios[]` | 场景列表 |
| `results[]` | 每个场景的延迟、错误、输出 intents |
| `conclusion.accepted` | 是否全部通过 |

## 如何读取/验证

- 用 `jq` 过滤 NDJSON：`jq 'select(.stage == "decision")' data/evaluation/runtime.ndjson`
- 用 Python `json` / `jsonlines` 读取 JSONL。
- 对 fixture 的回归验证：修改 `aios-spec` 类型后，必须同步更新 fixture 和 dataset tests。
- 新增数据集时，务必写入 `schema_version`，并让 `crates/aios-cli/tests/*_dataset_test.rs` 读取。

## 相关文档

- [数据流](../architecture/data-flow.md)
- [管线与运行时](../architecture/pipeline.md)
- [模型记忆与行为画像](../architecture/model-memory.md)
- [评估场景与数据集](../evaluation/scenarios.md)
- [评估工具](../evaluation/tools.md)
- `data/schemas/datapath.md`
