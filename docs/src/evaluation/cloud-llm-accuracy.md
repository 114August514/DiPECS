# Cloud LLM 准确率评估

本评估衡量云端模型对脱敏后的移动上下文能否返回预期的 DiPECS intent/action。与烟雾测试和延迟测试独立：每个用例都有标签，测试计算准确率。

## 数据来源

推荐的外部数据源：

| 数据源 | 适用性 | 在 DiPECS 中的用途 |
|--------|--------|---------------------|
| LSApp | 大规模移动应用使用轨迹；最适合 next-app/action 标签 | 将应用切换转换为 `foreground_apps`、`notified_apps`、粗粒度语义提示和预期动作 |
| MobileRec | 大规模应用推荐数据；包含应用包名、名称、分类、评分日期和用户-item 交互 | 将时间序列应用交互转换为 `CheckNotification`/`PreWarmProcess` 用例；使用应用分类作为语义提示 |
| StudentLife | 智能手机传感和日常上下文 | 将位置/活动/时间上下文转换为保守的 `Idle`、`KeepAlive` 或通知检查用例 |
| ExtraSensory | 手机/手表传感器的上下文识别标签 | 将通勤/居家/工作/活动等标签转换为系统/上下文用例 |
| 智能电池/应用使用轨迹 | 电池、网络、亮度、应用使用 | 将低电量/充电/网络状态转换为 `ReleaseMemory`、`KeepAlive` 或 `NoOp` 用例 |
| Mobile Data Challenge / 移动轨迹 | 位置和移动行为 | 将通勤/居家/工作上下文转换为预期的保守系统动作 |

已调研的 GitHub 候选数据源：

| 仓库 | 内容 | 实际状态 |
|------|------|----------|
| `aliannejadi/LSApp` | 顺序移动应用使用数据。README 报告 599,635 条使用记录、76,247 个会话、87 个不同应用、292 个用户。列包括 `user_id`、`session_id`、`timestamp`、`app_name`、`event_type`。 | 已作为 `third_party/LSApp` 引入；最佳首选外部源 |
| `mhmaqbool/mobilerec` | MobileRec 基线和数据集描述。README 描述 19.3M 用户-item 交互、0.7M 用户、10,173 个应用。 | 良好的第二来源，但需要单独下载/准备；适合应用分类和包级别的推荐用例 |
| `abebe198921-oss/3-days-battery-consumption-data` | 72 小时小型智能手机电池轨迹；CSV 包含亮度、应用使用和网络活动的功耗数据。 | 适合系统状态用例：低电量、充电、网络活动、release-memory/no-op |
| `Bhanu12318805/Mobile-Usage-Data-Analysis` | 移动使用 EDA，含人口统计、屏幕时间、数据使用、应用使用、游戏、流媒体和充值费用。 | 适合 persona 多样化，非直接下一步动作标签 |
| `taozeze/studentlyfe` | 围绕 Dartmouth StudentLife 数据集的 notebook 探索。 | 适合日常上下文/persona 多样化。需要外部 StudentLife 下载和单独转换器 |
| `Kushagra-Malani/Emotion-Prediction-using-ML` | 使用 ExtraSensory 手机/手表数据的示例项目。 | 上下文标签的有用参考；非直接应用使用数据集 |
| `alexcaselli/Federated-Learning-for-Human-Mobility-Models` | 使用多种移动数据集的移动建模项目。 | 可贡献通勤/居家/工作上下文用例，但非直接应用-动作标签 |

LSApp 之后最有价值的两个补充：

1. **MobileRec 转换器**：解析 `uid`、`app_package`、`app_name`、`app_category`、`unix_timestamp`，生成下一个应用/分类的预期 `CheckNotification`/`PreWarmProcess` 用例。
2. **电池/上下文转换器**：解析电池/网络/活动数据集，生成预期的 `ReleaseMemory`、`KeepAlive` 和 `NoOp` 用例，使模型超越应用切换预测的鲁棒性。

不要提交大型上游数据集，除非其许可证明确允许。
将其保留在 `third_party/` 或本地数据目录中，仅在适当时提交生成的、已脱敏的评估用例。


## 数据泄露警告

旧版 `hinted-action` 生成集适合作为 JSON 格式和动作转换的烟雾测试，但不是有效的准确率基准：它们从观测到的未来应用/分类中派生 `notified_apps`、`semantic_hints` 和 `file_activity`。这会将标签泄露到输入中，可能产生人为偏高的 Top-K 分数。

准确率报告请优先使用 LSApp 的 `--label-mode next-app`。此模式仅使用当前/近期应用历史作为输入，并在预测窗口内标注实际观测到的下一个应用。

## 从 LSApp 生成用例

先准备 LSApp：

```bash
git submodule update --init third_party/LSApp
bash tools/prepare-lsapp.sh
```

生成更大的云端准确率数据集：

```bash
python tools/generate/generate_cloud_accuracy_cases.py \
  --input data/lsapp/lsapp.tsv \
  --output data/evaluation/cloud-llm-accuracy-cases.generated.json \
  --max-cases 500 \
  --include-seed
```

生成的文件使用与 `data/evaluation/cloud-llm-accuracy-cases.json` 相同的 schema。


## 从其他来源生成用例

MobileRec 格式的应用交互文件：

```bash
python tools/generate/generate_cloud_accuracy_cases.py \
  --source-kind mobilerec \
  --input third_party/MobileRec \
  --output data/evaluation/cloud-llm-accuracy-cases.mobilerec.json \
  --max-cases 1000 \
  --include-seed
```

电池/应用使用遥测文件：

```bash
python tools/generate/generate_cloud_accuracy_cases.py \
  --source-kind battery \
  --input third_party/battery-usage \
  --output data/evaluation/cloud-llm-accuracy-cases.battery.json \
  --max-cases 300 \
  --include-seed
```

聚合移动使用/persona 文件：

```bash
python tools/generate/generate_cloud_accuracy_cases.py \
  --source-kind mobile-usage \
  --input third_party/mobile-usage \
  --output data/evaluation/cloud-llm-accuracy-cases.mobile-usage.json \
  --max-cases 300 \
  --include-seed
```

所有生成的文件可通过 `CLOUD_ACCURACY_CASES=<path>` 传递给 Rust 准确率测试。

## 运行准确率测试

### Rust 测试（小规模 seed 用例）

```bash
DIPECS_CLOUD_LLM_API_KEY=sk-xxx \
CLOUD_ACCURACY_ROUNDS=3 \
cargo test -p aios-agent --test cloud_accuracy_test -- --ignored --nocapture
```

### Rust 测试（大规模生成用例）

```bash
DIPECS_CLOUD_LLM_API_KEY=sk-xxx \
CLOUD_ACCURACY_CASES=data/evaluation/cloud-llm-accuracy-cases.generated.json \
CLOUD_ACCURACY_ROUNDS=3 \
CLOUD_ACCURACY_MIN_PCT=90 \
cargo test -p aios-agent --test cloud_accuracy_test -- --ignored --nocapture
```

### Python Top-K 评估器

独立的 Python 评估工具，支持多种匹配模式和数据集：

```bash
# Seed 用例，core 匹配模式（intent + action + extension）
python tools/evaluate/evaluate_cloud_accuracy.py \
  --cases data/evaluation/cloud-llm-accuracy-cases.json \
  --rounds 2 \
  --match-mode core \
  --out-dir data/evaluation

# LSApp action-intent 用例
python tools/evaluate/evaluate_cloud_accuracy.py \
  --cases data/evaluation/cloud-llm-accuracy-cases.lsapp-action-intent.json \
  --rounds 1 \
  --match-mode action \
  --limit-cases 50 \
  --out-dir data/evaluation
```

匹配模式说明：

| 模式 | 比较字段 | 适用场景 |
|------|----------|----------|
| `full` | intent_type + action_type + target + extension_category | 严格评估，target 格式必须精确匹配 |
| `core` | intent_type + action_type + extension_category | 忽略 target 格式差异（`pkg:com.x` vs `com.x`），开发迭代用 |
| `action` | 仅 action_type | 仅评估动作类型，适用于 next-app / action-intent 任务 |

输出文件格式（schema v2）：

```text
data/evaluation/cloud-accuracy-topk-<unix_timestamp>.json
```

关键字段：

- `results.top1_accuracy_pct` / `top3_accuracy_pct` / `top5_accuracy_pct`：各级 Top-K 准确率
- `results.success_rate_pct`：成功调用率（排除 API 错误）
- `results.latency_p50_ms` / `latency_p95_ms`：延迟分位数
- `reference.top5_meets_reference`：是否达到基准线（默认90%）
- `case_results[].rank`：预期决策在 Top-K 中的排名（`null`=未命中）

## 用例文件说明

| 文件 | 用例数 | 标签模式 | 说明 |
|------|--------|----------|------|
| `cloud-llm-accuracy-cases.json` | 30 | hinted-action | 手工编写的 seed 用例，覆盖多种 persona 和场景 |
| `cloud-llm-accuracy-cases.battery.json` | 300 | hinted-action | 从电池/应用使用数据生成 |
| `cloud-llm-accuracy-cases.lsapp.json` | 1000 | hinted-action | 从 LSApp 生成（注意：有数据泄露风险，仅用于烟雾测试） |
| `cloud-llm-accuracy-cases.lsapp-next-app.json` | 100 | next-app | 从 LSApp 生成，无数据泄露，适合准确率报告 |
| `cloud-llm-accuracy-cases.lsapp-action-intent.json` | 100 | action-intent | 从 LSApp 生成，仅评估 action_type |
