# 评估工具

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `tools/collect-*.sh`, `tools/generate_synthetic_android_trace.py`, `crates/aios-cli/tests/*_dataset_test.rs`

**这篇文档回答什么**：如何运行 DiPECS 的评估工具，生成资源、UX、稳定性数据集以及合成 trace。  
**适合谁读**：需要在 Android 模拟器或真机上复现评估结果、生成 fixture 的人。

## TL;DR

`tools/` 下的脚本都是**手动测量工具**，产出结构化 JSON 数据集；CI 再通过 `crates/aios-cli/tests/*_dataset_test.rs` 校验这些数据集是否满足阈值。合成 trace 工具则不需要真机。

| 工具 | 需要什么 | 输出 |
| --- | --- | --- |
| `collect-resource-overhead.sh` | Android 模拟器/真机 | 资源开销数据集 |
| `collect-ux-metrics.sh` | Android 模拟器/真机 | UX 数据集 |
| `collect-stability.sh` | Android 模拟器/真机 | 稳定性数据集 |
| `generate_synthetic_android_trace.py` | 仅 Python | 合成脱敏 JSONL trace |

## 前置条件

- Android 模拟器已启动，或真机已通过 adb 连接。
- 已安装目标包 `com.dipecs.collector`。
- `adb` 在 PATH 中（脚本会自动检测 Windows 下的 `adb.exe`）。
- Python 3 已安装（用于合成 trace 和 action sender）。

通用环境变量：

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `ADB` | `adb` | adb 路径 |
| `PACKAGE` | `com.dipecs.collector` | 目标包名 |
| `OUT_DIR` | `data/evaluation` | 输出目录 |
| `TOKEN` | `dipecs-dev-emulator-shared-token-00000000` | action socket HMAC token |
| `PORT` | `46321` | bridge 端口 |
| `ACTION_HOST` | `127.0.0.1` | bridge 主机 |

## 资源开销：`collect-resource-overhead.sh`

测量三种模式下的 CPU、RSS、PSS、电池/温度估算和 jank：

- `baseline_idle`：app 被 force-stop
- `dipecs_observe_only`：仅采集
- `dipecs_action_loop`：采集 + 动作回路

### 运行

```bash
./tools/collect-resource-overhead.sh
```

### 可调环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `SAMPLES_PER_MODE` | `10` | 每种模式采样次数 |
| `SAMPLE_INTERVAL_SECS` | `10` | 采样间隔 |

### 输出

- `data/evaluation/resource-overhead-emulator-<ts>.json`
- `data/evaluation/resource-overhead-emulator-<ts>.md`

### 对应 CI 测试

`crates/aios-cli/tests/resource_overhead_dataset_test.rs`

阈值：

- CPU delta ≤ 8 个百分点
- PSS delta ≤ 80 MB

## UX 指标：`collect-ux-metrics.sh`

测量 `PreWarmProcess` 启动加速和 `ReleaseMemory` 对 jank / 内存的影响。

采集模式：

- `no_dipecs_baseline`
- `cold_startup`
- `prewarm_startup`
- `baseline_jank`
- `post_release_jank`

### 运行

```bash
./tools/collect-ux-metrics.sh
```

### 输出

- `data/evaluation/ux-metrics-emulator-<ts>.json`
- `data/evaluation/ux-metrics-emulator-<ts>.md`

### 对应 CI 测试

`crates/aios-cli/tests/ux_metrics_dataset_test.rs`

阈值：

- PreWarm 加速 ≥ 20% 或 ≥ 100 ms
- ReleaseMemory jank 增加 ≤ 20 个百分点

## 稳定性：`collect-stability.sh`

长时间采样 RSS/PSS/CPU，用线性回归判断内存泄漏。

### 运行

```bash
# 默认 10 分钟
./tools/collect-stability.sh

# 标准长运行
DURATION_MINUTES=60 SAMPLE_INTERVAL_SECS=30 ./tools/collect-stability.sh
```

### 可调环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `DURATION_MINUTES` | `10` | 总时长 |
| `SAMPLE_INTERVAL_SECS` | `30` | 采样间隔 |

### 输出

- `data/evaluation/stability-emulator-<ts>.jsonl`（原始样本）
- `data/evaluation/stability-emulator-<ts>.json`（汇总数据集）

### 对应 CI 测试

`crates/aios-cli/tests/stability_dataset_test.rs`

阈值：

- RSS 增长 ≤ 50 MB/h
- PSS 增长 ≤ 20 MB/h
- 平均 CPU ≤ 10%

## 合成 Trace：`generate_synthetic_android_trace.py`

不需要真机，生成确定性、已脱敏的 Android JSONL trace，用于 replay、策略测试和文档示例。

### 运行

```bash
python3 tools/generate_synthetic_android_trace.py \
  --rows 2400 \
  --output data/traces/android_synthetic_large.redacted.jsonl \
  --summary data/traces/android_synthetic_large.redacted.summary.json \
  --seed 20260628
```

### 参数

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `--rows` | `2400` | JSONL 行数 |
| `--output` | `data/traces/...` | 输出 JSONL |
| `--summary` | `data/traces/...` | 输出统计摘要 |
| `--seed` | `20260628` | 随机种子，保证可复现 |

### 输出

- JSONL trace，事件类型包括 `app_transition`、`notification_posted`、`notification_interaction`、`context_heartbeat`、accessibility 事件、`screen_state` 等。
- Summary JSON，含 `eventTypeCounts`、`sourceCounts`、`rawEventKindCounts`。

注意：输出明确标记为 `synthetic` 和 `redacted`，不能作为真实设备证据。

## Trace Dashboard

`tools/trace-dashboard/index.html` 是一个静态 HTML 页面，可在浏览器中浏览 JSON/JSONL/NDJSON trace 文件。

用法：

```bash
# 用任意静态服务器打开
python3 -m http.server 8080 --directory tools/trace-dashboard
# 然后访问 http://localhost:8080
```

## 与 CI 的关系

| 工具 | CI 是否直接运行 | CI 如何验证 |
| --- | --- | --- |
| `collect-resource-overhead.sh` | 否 | dataset test 读取已提交的 fixture |
| `collect-ux-metrics.sh` | 否 | dataset test 读取已提交的 fixture |
| `collect-stability.sh` | 否 | dataset test 读取已提交的 fixture |
| `generate_synthetic_android_trace.py` | 否 | replay/golden/noop 测试使用 fixture |

因此：**工具产出的 fixture 必须提交到 git，CI 才能做回归验证**。

## 常见问题

### 脚本提示找不到 `action-forensic-sender.py`

确保 `tests/scenarios/lib/action-forensic-sender.py` 存在；该 sender 用于向 action socket 注入动作。

### 资源开销/UX 脚本在 Windows 上找不到 Python

脚本会尝试检测 Windows Python 3.13；可通过 `PYTHON` / `SEND_PYTHON` 环境变量显式指定。

### 新 fixture 加入后 CI 失败

dataset tests 会从原始样本重新计算 summary。如果新 fixture 内部不一致或超出阈值，测试会失败。此时应检查数据采集过程，而不是简单修改测试阈值。

## 相关文档

- [评估场景与数据集](../evaluation/scenarios.md)
- [调试指南](../team/debugging.md)
- [Schema 参考](../refs/schemas.md)
