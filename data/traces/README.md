# Trace Fixtures

本目录存放 replay、dashboard、golden、policy/audit 验证用的 JSONL 轨迹文件。

## 目录结构

```
data/traces/
├── sample_replay.jsonl                         最简单的回放样本（8 行）
├── denial.jsonl                                Policy denial 验证（3 行）
├── noop_matrix.jsonl                           NoOp 行为覆盖矩阵
├── golden_sample.json                          Golden trace schema 参照
├── android_real_device_sample.redacted.jsonl   真机采集脱敏样本（5 行）
├── android_synthetic_large.redacted.jsonl      旧版合成轨迹，通知已脱敏（2400 行）
├── emulator-e2e-20260630-022449.jsonl          模拟器端到端采集（批次 1）
├── emulator-e2e-20260630-022555.jsonl          模拟器端到端采集（批次 2）
└── scenarios/                                  场景化合成轨迹（含真实通知文本）
    ├── synthetic_scenarios_index.json          场景索引
    ├── rich-workflow.jsonl                    多应用真实会话（2000 行）
    ├── low-battery.jsonl                      低电量触发 ReleaseMemory（600 行）
    ├── privacy-sensitive.jsonl                密集验证码通知 → 本地路由（400 行）
    ├── morning-routine.jsonl                  重复日常模式 → 模型记忆测试（1200 行）
    ├── circuit-breaker.jsonl                  高频通知 → 熔断测试（300 行）
    └── multi-app-switching.jsonl              频繁前后台切换（1000 行）
```

## 根目录 — 核心验证 trace

| 文件 | 行数 | 用途 |
|------|------|------|
| `sample_replay.jsonl` | 8 | 基础回放：7 个 rawEvent + 1 个 Accessibility 空行 |
| `denial.jsonl` | 3 | Policy 拒绝场景：target-not-in-context / capability-denied |
| `noop_matrix.jsonl` | - | 各场景 NoOp 覆盖率矩阵 |
| `golden_sample.json` | - | Golden trace：raw event → expected sanitized 对照 |
| `android_real_device_sample.redacted.jsonl` | 5 | 真机脱敏样本，验证 PublicApi ingress |
| `android_synthetic_large.redacted.jsonl` | 2400 | 旧版合成轨迹，通知 title/text 已脱敏为空 |
| `android_synthetic_large.redacted.summary.json` | - | 上表合成轨迹的统计摘要（事件类型/来源计数），由 `generate_synthetic_android_trace.py` 产出 |
| `synthetic-next-app-v1.labels.jsonl` | - | next-app 基准的 ground-truth 标签（每窗口的实际下一个 app），供 `benchmark_next_app` 使用 |

## 运行捕获产物（时间戳命名，非固定 fixture）

以下文件是端到端场景脚本（`tests/scenarios/lib/*-stages.sh`）在模拟器/真机运行时**捕获的原始 trace**，文件名带时间戳。它们是可追溯的运行证据，不被任何测试按精确路径引用；新增运行会产生新文件。

| 文件模式 | 来源脚本 | 说明 |
|------|------|------|
| `emulator-e2e-<ts>.jsonl` | `emulator-e2e-stages.sh` | 模拟器端到端采集原始数据（每次运行一个批次） |
| `action-loop-e2e-<ts>.jsonl` | `action-loop-stages.sh` | 动作回路端到端运行捕获 |
| `action-type-coverage-<ts>.jsonl` | action-loop 覆盖运行 | 四类可转发动作覆盖捕获 |

## scenarios/ — 场景化合成轨迹

与旧版合成轨迹的关键区别：**通知包含真实文本**（文件分享、验证码、金融交易、链接、图片等），能触发 `PrivacyAirGap` 的语义提示提取。

| 场景 | 行数 | 摄入 | 说明 |
|------|------|------|------|
| `rich-workflow` | 2000 | 1711 | 多应用真实会话，含 FileMention / VerificationCode / FinancialContext / ImageMention / LinkAttachment |
| `low-battery` | 600 | 548 | 电量从 30% 降至 3%，触发 ReleaseMemory / KeepAlive |
| `privacy-sensitive` | 400 | 400 | 密集验证码+金融通知，触发隐私敏感路由 → 纯本地推理 |
| `morning-routine` | 1200 | 1026 | 5 组重复日常模式，用于模型记忆/行为画像测试 |
| `circuit-breaker` | 300 | 300 | 高频通知轰炸，测试熔断器和云端回退路径 |
| `multi-app-switching` | 1000 | 1000 | 6 个应用间频繁前后台切换 |

回放示例：

```bash
cargo run -p aios-cli -- replay data/traces/scenarios/rich-workflow.jsonl \
  --stages policy \
  --audit data/evaluation/rich-workflow.audit.ndjson
```
