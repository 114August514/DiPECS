# 评估场景与数据集

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `tests/scenarios/`, `tools/collect-*.sh`, `crates/aios-cli/tests/*_dataset_test.rs`

**这篇文档回答什么**：DiPECS 如何验证功能正确性、资源开销、UX 影响和长期稳定性。  
**适合谁读**：想复现评估结果、理解 CI 阈值，或者新增评估场景的人。

## TL;DR

DiPECS 的评估分两条线：

- **离线回归**：基于 fixture / dataset 的 cargo test，CI 每次运行。
- **端到端场景**：在 Android 模拟器或真机上跑 shell 脚本，产出结构化报告。

本地开发优先跑 `cargo test --workspace`；涉及 Android 采集或 action-loop 时再跑场景脚本。

## 何时读这篇

| 场景 | 看哪一节 |
| --- | --- |
| 想知道 CI 检查哪些指标 | CI 离线回归 |
| 想跑端到端验证 | 端到端场景脚本 |
| 想生成资源/UX/稳定性数据集 | 评估工具 |
| 想了解 `data/evaluation/` 里的文件 | data/evaluation 产物说明 |
| 要新增一个场景 | 如何新增一个场景 |

## 两条评估线总览

```text
离线回归（CI）
  ├─ privacy_leak_test
  ├─ golden hash replay tests
  ├─ dataset regression tests
  └─ cloud LLM mock tests

端到端（模拟器/真机）
  ├─ emulator-e2e.sh
  ├─ action-loop-e2e.sh
  ├─ action-latency-sweep.sh
  └─ on-device-dipecsd.sh
```

## CI 离线回归

### dataset tests

| 测试文件 | 验证目标 |
| --- | --- |
| `resource_overhead_dataset_test.rs` | CPU / PSS 增量 |
| `ux_metrics_dataset_test.rs` | PreWarm 加速 / ReleaseMemory jank 影响 |
| `stability_dataset_test.rs` | 内存泄漏 |

### 阈值

| 维度 | 阈值 | 测试 |
| --- | --- | --- |
| CPU delta | ≤ 8 个百分点 | `resource_overhead_fixture_stays_within_budget` |
| PSS delta | ≤ 80 MB | `resource_overhead_fixture_stays_within_budget` |
| PreWarm 加速 | ≥ 20% 或 ≥ 100 ms | `ux_metrics_prewarm_shows_no_regression` |
| Jank 增加 | ≤ 20 个百分点 | `ux_metrics_release_memory_does_not_increase_jank` |
| 内存泄漏 | ≤ 50 MB/h RSS | `stability_no_memory_leak` |

### 云端决策基准（需 API key）

```bash
cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::smoke -- --ignored --nocapture
cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::latency -- --ignored --nocapture
```

## 端到端场景脚本

`tests/scenarios/`：

| 脚本 | 目的 | 何时跑 |
| --- | --- | --- |
| `emulator-e2e.sh` | 模拟器采集链路验证 | 修改 Android 采集或 Rust 脱敏后 |
| `action-loop-e2e.sh` | 真机/模拟器动作回路验证 | 修改 action-loop、HMAC、bridge 后 |
| `action-latency-sweep.sh` | 四种可转发动作延迟扫描 | 验证动作性能 |
| `on-device-dipecsd.sh` | 设备内直接运行 `dipecsd` | 验证无 adb forward 的闭环 |

公共约定：

- `pin_serial()` 自动选择设备或要求 `ANDROID_SERIAL`。
- 运行前 `pm clear` 避免旧 trace 污染。
- `classify_action_state()` 输出 `EXECUTED` / `SCHEDULED` / `REJECTED` / `NOT-EXECUTED`。
- `redaction_leak_sample()` 检查敏感 key 是否已被 redact。

## 评估工具

`tools/`：

| 脚本 | 测量内容 | 输出 |
| --- | --- | --- |
| `collect-resource-overhead.sh` | CPU、RSS、PSS、电池/温度估算、jank | `data/evaluation/resource-overhead-emulator-*.json` + `.md` |
| `collect-ux-metrics.sh` | PreWarm 启动加速、ReleaseMemory jank 降低、内存使用 | `data/evaluation/ux-metrics-emulator-*.json` + `.md` |
| `collect-stability.sh` | 长时间内存稳定性 | `data/evaluation/stability-emulator-*.json` |
| `generate_synthetic_android_trace.py` | 确定性脱敏合成 trace | `data/traces/android_synthetic_large.redacted.jsonl` |

`collect-stability.sh` 阈值：

```json
{
  "thresholds": {
    "max_rss_growth_per_hour_mb": 50.0,
    "max_pss_growth_per_hour_mb": 20.0,
    "max_avg_cpu_pct": 10.0
  }
}
```

## data/evaluation 产物说明

| 文件 | 内容 |
| --- | --- |
| `emulator-e2e-*.md` / `.ndjson` / `.audit` | 模拟器 E2E replay 结果、audit hash |
| `action-loop-e2e-*.md` | 动作回路结果、状态、forward 次数 |
| `action-type-coverage-*.md` | 四种可转发动作全部执行过的证据 |
| `on-device-dipecsd-*.md` | 设备内 daemon 闭环结果 |
| `resource-overhead-emulator-*.json` / `.md` | 资源开销数据集 |
| `ux-metrics-emulator-*.json` / `.md` | UX 数据集 |
| `stability-emulator-canonical.json` | 稳定性回归 canonical 数据集 |
| `value-metrics-*.md` | 综合价值报告 |
| `cloud-latency-*.json` | 云端 LLM 延迟基准 |
| `cloud-scenarios-*.json` | 云端 LLM 场景 smoke 结果 |

## 如何新增一个场景

1. 在 `tests/scenarios/` 新增脚本，复用 `lib/` 里的 `pin_serial`、`classify_action_state` 等 helper。
2. 在 `tools/` 添加数据收集脚本（如果需要新指标）。
3. 在 `crates/aios-cli/tests/` 新增 dataset test，把阈值写进代码。
4. 把生成的 fixture 放入 `data/evaluation/`，并在 CI 中引用。
5. 更新本文档的表格和 [调试指南](../team/debugging.md) 的常见故障模式。

## 相关文档

- [模拟器评估套件](emulator-evaluation-suite.md)
- [动作执行与 Android bridge](../architecture/action-execution.md)
- [云端 LLM 后端](../architecture/cloud-llm.md)
- [调试指南](../team/debugging.md)
