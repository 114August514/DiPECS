# LSApp Next-App 预测评估结果

> 测试者：lsy
> 日期：2026-07-04
> 数据集：LSApp（3,658,590 行，87 个不同应用，约 50-100 个用户，2018-01）
> 测试配置：horizon_secs=30, history_len=5, top-k=5
> 环境：WSL，Rust release 模式，cargo build --release

## 标准分割（按用户 80/20 时间分割）

- 训练样本数：1,089,459
- 测试样本数：272,519

| 排序器 | hit@1 | hit@3 | hit@5 | MRR@5 |
|--------|-------|-------|-------|-------|
| global_popularity | 13.585% | 32.538% | 45.892% | 0.248 |
| mfu | 28.041% | 58.654% | 73.821% | 0.447 |
| mru | 23.524% | 23.524% | 23.524% | 0.235 |
| naive_bayes | 38.007% | 57.528% | 66.896% | 0.487 |
| markov | 39.624% | 66.318% | 77.154% | 0.538 |
| xgboost (feature_lift) | 33.441% | 51.712% | 60.711% | 0.435 |
| strong_predictive | 53.784% | 72.563% | 80.428% | 0.638 |
| adaptive_predictive | 52.378% | 69.487% | 78.299% | 0.619 |
| **ensemble** | **56.509%** | **76.059%** | **84.588%** | **0.671** |

## 冷启动分割（80/20 用户分割，无用户重叠）

- 训练样本数：977,842
- 测试样本数：384,136

| 排序器 | hit@1 | hit@3 | hit@5 | MRR@5 |
|--------|-------|-------|-------|-------|
| global_popularity | 11.778% | 23.328% | 46.848% | 0.224 |
| mfu | 11.778% | 23.328% | 46.848% | 0.224 |
| mru | 31.495% | 31.495% | 31.495% | 0.315 |
| naive_bayes | 9.983% | 24.054% | 32.727% | 0.179 |
| markov | 17.901% | 35.492% | 45.172% | 0.278 |
| xgboost (feature_lift) | 27.047% | 41.443% | 46.954% | 0.346 |
| strong_predictive | 48.050% | 58.263% | 63.724% | 0.537 |
| adaptive_predictive | 48.813% | 61.489% | 67.871% | 0.558 |
| **ensemble** | **50.446%** | **61.384%** | **66.335%** | **0.562** |

## 与上次运行的差异

已提交报告（prior）vs 本次运行：

### 标准分割

| 排序器 | 指标 | 旧版 | 本次运行 | 差异 |
|--------|------|------|----------|------|
| ensemble | hit@1 | 56.442% | 56.509% | +0.07 |
| ensemble | hit@3 | 76.104% | 76.059% | -0.05 |
| ensemble | hit@5 | 84.241% | 84.588% | +0.35 |
| strong_predictive | 全部 | (相同) | (相同) | 0.00 |

### 冷启动分割

| 排序器 | 指标 | 旧版 | 本次运行 | 差异 |
|--------|------|------|----------|------|
| ensemble | hit@1 | 21.196% | 50.446% | +29.25 |
| ensemble | hit@3 | 40.852% | 61.384% | +20.53 |
| ensemble | hit@5 | 52.567% | 66.335% | +13.77 |
| strong_predictive | 全部 | (相同) | (相同) | 0.00 |

冷启动 ensemble 的提升来自新增的 `markov_context`（时序 Markov）和 `adaptive_predictive` 组件，它们不依赖于单用户历史。

## 新增排序器（此前报告中未出现）

- **adaptive_predictive**：独立 RRF，组合 StrongPredictiveActionBaseline + Markov3（3阶）+ TimeMarkov（时序）。手动调优权重：strong=1.0, mark3=0.8, time=0.6, popularity=0.01。
- **markov_context**：新增到 ensemble 的时序 Markov 组件。以 `(current, hour_bucket)` 和 `(current, weekday)` 为键进行状态转移。

## Cloud LLM 准确率评估

使用 DeepSeek API（`deepseek-v4-pro`）评估 Cloud LLM 决策准确率。

### Seed 用例（30 条手工用例）

| 匹配模式 | Top-1 | Top-3 | Top-5 | 成功率 |
|----------|-------|-------|-------|--------|
| full（intent+action+target+extension） | 63.3% | 66.7% | 66.7% | 100% |
| core（intent+action+extension，忽略 target 格式） | 65.0% | 76.7% | 76.7% | 100% |

### LSApp action-intent 用例（50 条）

| 匹配模式 | Top-1 | 成功率 |
|----------|-------|--------|
| action（仅 action_type） | 42.0% | 100% |

### LSApp next-app 用例（30 条）

| 匹配模式 | Top-1 | 成功率 |
|----------|-------|--------|
| full | 13.3% | 100% |

> 注：Cloud LLM 在 next-app 任务上表现较差（13.3%），远低于本地 ensemble 的 56.5%。action-intent 任务（42%）也低于本地模型。这表明当前 prompt 和任务框架需要进一步优化。
