# 合成下一应用预测基准

本评测从现有 Git LFS 合成 Android 轨迹中派生下一应用标签，在完全相同的脱敏上下文上分别运行 `RuleBasedBackend` 与 `LocalEvaluatorBackend`。评测用于验证两种本地启发式方法在设计场景中的相对表现，**不代表真实用户预测准确率**。

---

## 数据来源

使用三组确定性合成轨迹：

| 场景 | 原始行数 | 场景特征 |
|---|---:|---|
| `multi-app-switching` | 1000 | 高频应用前后台切换 |
| `morning-routine` | 1200 | 重复日常行为和通知模式 |
| `rich-workflow` | 2000 | 多应用、通知、设备状态和动作事件 |

这些文件均标记为 `synthetic`，不能作为真实设备或真实用户行为证据。原始 LFS 轨迹保持不变，派生标签位于 `data/traces/synthetic-next-app-v1.labels.jsonl`。

---

## Ground Truth 构造方法

1. 解析有效 `rawEvent`，按 `timestamp_ms + eventId + 原始行号` 排序，修复文件行顺序与事件时间可能不一致的问题。
2. 使用固定 10 秒上下文窗口 `[start, end)`。
3. 将窗口结束前最后一次 `Foreground` 应用定义为 `current_app`。
4. 在窗口结束后的 30 秒内寻找第一次切换到、且不同于 `current_app` 的前台应用，定义为 `actual_next_app`。
5. 重复上报相同前台应用不算新切换；未来 30 秒无不同应用则标记为 `no_future_switch`。
6. 仅当 `actual_next_app` 已在当前窗口作为前台应用或通知来源出现时，样本才进入主准确率统计。这与 PolicyEngine 的 `target-in-context` 约束一致。
7. 当前已经前台的应用从预测候选中删除，避免将“当前应用继续前台”误算为下一应用命中。

每条标签均保存窗口、当前应用、可观察候选、未来真实应用、时间和排除原因。标签生成重复运行的 SHA-256 一致：

```text
835b049c10b7c1999edd983e27fd2bf8d67d5528b5e89a790a1462f408ae4d67
```

---

## 运行命令

```bash
cargo run -p aios-cli -- benchmark-next-app \
  --input data/traces/scenarios/multi-app-switching.jsonl \
  --input data/traces/scenarios/morning-routine.jsonl \
  --input data/traces/scenarios/rich-workflow.jsonl \
  --labels data/traces/synthetic-next-app-v1.labels.jsonl \
  --report data/evaluation/synthetic-next-app-v1.report.json
```

两个后端直接处理同一个 `StructuredContext`，不经过 `DecisionRouter`，避免路由决策干扰算法对比。候选应用从 `OpenApp`、`SwitchToApp`、`CheckNotification` 和 `PreWarmProcess(pkg:...)` 中提取，并按 confidence 降序排列。现有权重未根据本基准调整。

---

## 数据集规模

| 指标 | 数值 |
|---|---:|
| 上下文窗口总数 | 946 |
| 存在未来不同应用切换 | 764 |
| 满足 context-supported 条件 | 178 |
| Context-supported switch coverage | **23.298%** |
| 标签在当前上下文不可观察 | 586 |
| 未来 30 秒无不同应用切换 | 182 |

只有 23.298% 的未来切换满足当前 `target-in-context` 条件。这说明系统当前只能对有限的、已有上下文证据的切换进行受控预测，不能覆盖任意下一应用。

---

## 聚合结果

| 指标 | RuleBased | LocalEvaluator |
|---|---:|---:|
| Top-1 Accuracy | **61.236%** | 43.820% |
| Top-3 Accuracy | **65.730%** | 62.921% |
| Prediction Coverage | **93.820%** | 73.596% |
| Conditional Top-1 Accuracy | **65.269%** | 59.542% |
| Wrong Prediction Rate | **34.731%** | 40.458% |
| No-prediction Rate | **6.180%** | 26.404% |
| Mean Reciprocal Rank | **0.635** | 0.531 |
| Macro Top-1 Accuracy | **60.704%** | 43.823% |
| Macro Top-3 Accuracy | **65.225%** | 64.309% |

当前合成场景中，RuleBased 的 Top-1、覆盖率和错误率均优于 LocalEvaluator。LocalEvaluator 的 Macro Top-3 与 RuleBased 接近，说明正确应用较常出现在候选集合中，但候选排序和预测覆盖仍需改进。

---

## 分场景结果

| 场景 | 有效样本 | Rule Top-1 | Rule Top-3 | Local Top-1 | Local Top-3 |
|---|---:|---:|---:|---:|---:|
| `multi-app-switching` | 52 | 51.923% | 57.692% | 44.231% | **88.462%** |
| `morning-routine` | 65 | **64.615%** | **70.769%** | 44.615% | 56.923% |
| `rich-workflow` | 61 | **65.574%** | **67.213%** | 42.623% | 47.541% |

在高频切换场景中，LocalEvaluator 的 Top-3 达到 88.462%，但 Top-1 仅为 44.231%。这进一步表明其主要问题是候选排序，而不是完全无法发现相关应用。

---

## 如何解释结果

### 可以支持的结论

- 两个本地后端都能在相同脱敏上下文上产生可度量的应用候选。
- 在当前三组合成场景中，RuleBased 的 Top-1 表现优于 LocalEvaluator。
- LocalEvaluator 更保守，未预测率为 26.404%。
- LocalEvaluator 在 `multi-app-switching` 的 Top-3 较高，存在进一步优化排序的空间。
- 当前策略约束只覆盖约 23.3% 的未来不同应用切换。

### 不能支持的结论

- 不能称为真实用户预测准确率。
- 不能证明 RuleBased 在真实环境中一定优于 LocalEvaluator。
- 不能根据这组数据调参后继续把同一数据作为独立测试集。
- 不能把 Top-1 准确率直接等同于启动时间提升。
- 不能用该数据证明系统已经具备通用下一应用预测能力。

---

## 测试与可复现性

新增测试覆盖：

- 时间乱序输入的确定性处理；
- `[start, end)` 窗口边界；
- 重复 Foreground 不生成虚假切换；
- 当前应用从候选中剔除；
- 标签必须得到当前上下文支持；
- 相同输入重复生成字节一致标签。

相关验证通过：

```text
next_app_benchmark unit tests: 6 passed
replay_jsonl_test:             7 passed
golden_trace_integration_test: 6 passed
noop_rate_test:                1 passed
cargo fmt --check:             passed
cargo clippy -D warnings:      passed
```

完整机器可读报告位于 `data/evaluation/synthetic-next-app-v1.report.json`。

---

## PPT 推荐表述

> 我们从三组合成 Android 轨迹中构造了 946 个十秒上下文窗口，其中 178 个满足“未来下一应用已在当前上下文出现”的安全约束。在这些 context-supported 样本上，RuleBased Top-1 为 61.2%，LocalEvaluator 为 43.8%；LocalEvaluator 的 Top-3 为 62.9%。结果表明当前规则基线更稳定，而本地评分器需要改进候选排序和覆盖率。该实验仅为合成基准，不代表真实用户准确率。

