# 学术 Baseline 对比

> 状态：维护中
>
> 最后更新：2026-07-04
>
> 数据来源：`data/evaluation/next-app/academic-next-app-baselines.json`

本页回答一个很具体的评审问题：DiPECS 的 next-app 预测结果，应当如何与 MAPLE、AppFormer、POI-based prediction、HAPP/APPredict 系列、GNN 推荐模型等学术工作放在一起看？

简短结论是：当前仓库里，只有 DiPECS ensemble 与 `strong_predictive` baseline 使用同一个 LSApp Standard 报告、同一批测试窗口和同一套指标实现，因此可以直接比较。外部论文数字只能作为研究背景，除非我们在本仓库中复现了相同的数据转换、split、候选集和指标实现，否则不能写成 DiPECS 对这些方法的直接胜负结论。

## 如何阅读表格

`direct` 表示该行使用和 DiPECS 相同的提交报告、测试窗口、候选/标签管线和指标实现。

`contextual_only` 表示论文方向相关、指标名称相近，但数据集、预处理、split、候选集或指标定义至少有一项不同，只能作为背景材料。

`excluded_unclear` 表示来源相关，但暂时没有可验证的兼容数字，或该数字对应的是另一个任务。

只有当来源本身是“单个真实 next app”的 Top-K 预测任务时，本页才把 HR@K 与 Hit@K 视为同一类 top-K hit rate。MRR 在本页按百分比展示；例如 DiPECS 报告 JSON 中的 `0.670` 在本表中写作 `67.0%`。

## DiPECS 直接参照

| 方法 | 数据集 | Split | Hit@1 | Hit@3 | Hit@5 | MRR@5 | Macro Hit@1 | 可比性 |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| DiPECS ensemble | LSApp | Standard | 56.509% | 76.059% | 84.588% | 67.1% | 52.981% | 直接参照 |
| Strong predictive baseline | LSApp | Standard | 53.784% | 72.563% | 80.428% | 63.8% | 49.696% | 直接可比 |

这两行来自 `data/evaluation/next-app/lsapp-standard.report.json`。目前仓库里的准确率主张只应基于这两行：DiPECS ensemble 在同一 LSApp Standard 测试窗口上高于 `strong_predictive` baseline。

## 学术结果矩阵

| 方法 | 数据集 / split | Hit@1 | Hit@3 | Hit@5 | MRR@5 | 可比性 | 来源说明 |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- |
| MAPLE | LSApp，按用户时间顺序 70/10/20 | 71.57% | 86.49% | 91.50% | 79.36% | 仅作背景 | TGT Table II 引用了 MAPLE 原论文数值，未给标准差。 |
| MAPLE | LSApp，cold-start 用户级 90/10 | 76.44% | 88.48% | 92.47% | 82.72% | 仅作背景 | cold-start 是 unseen-user 协议，不是 DiPECS Standard split。 |
| TGT | LSApp，按用户时间顺序 70/10/20 | 81.15% | 94.94% | 96.67% | 88.07% | 仅作背景 | 使用同一公开数据集家族，但预处理和模型管线不同。 |
| TGT | LSApp，cold-start 用户级 90/10 | 82.02% | 95.46% | 96.71% | 88.76% | 仅作背景 | 可用于了解相关论文报告水平，但不能直接比较 DiPECS Standard。 |
| AppFormer + K + T | 上海运营商 app usage + POI，PAULCI 对照 partition | 31.92% | 52.13% | 61.05% | - | 仅作背景 | Table V 报告 Hit@K；其 MRR 口径没有作为 DiPECS MRR@5 写入。 |
| AppFormer + K + T | 上海运营商 app usage + POI，DUGN 对照 partition | 42.68% | 62.30% | 69.60% | 53.03% | 仅作背景 | Table VI 报告 Hit@K/MRR@K，但数据集和 split 与 DiPECS 不同。 |
| PAULCI | 上海运营商 app usage + POI | 27.17% | 47.53% | 57.32% | - | 仅作背景 | AppFormer Table V 中的 POI/location context baseline。 |
| POI transfer-learning popularity | 上海 city-scale 运营商数据 | - | - | - | - | 暂不纳入比较 | 原文报告 location-level top-five popular-app hit rate 83.00%，不是个人 next-app 窗口预测。 |
| GNN + self-attention | Tsinghua App Usage，5/1/1 天 split | - | - | - | - | 暂不纳入比较 | 已确认数据集、split 和指标族，但暂未把可验证数值写入 fixture。 |
| HAPP / APPredict family | 未确认 | - | - | - | - | 暂不纳入比较 | 只作为跟踪行；拿到主来源前不写数值主张。 |

## 对比准则

可以这样使用：

- 用 DiPECS ensemble vs `strong_predictive` 两行支撑当前仓库内的准确率结论。
- 把 MAPLE、TGT、AppFormer、POI、GNN 行作为相关工作背景。
- 每个数值都保留 `source_url` 和 `source_locator`。
- 新增外部论文行时，先更新 `data/evaluation/next-app/academic-next-app-baselines.json`，再同步更新本页。

不要这样使用：

- 不要仅凭本表声称 DiPECS 胜过或弱于 MAPLE、TGT、AppFormer、POI 或 GNN-based models。
- 不要混用 LSApp Standard 与 cold-start 结果。
- 不要把 location-level top-five app popularity 当作 next-app Hit@5。
- 不要从记忆或二手摘要里补数字；没有来源定位的数字宁可留空。

## 维护规则

每次更新都必须保持这些约束：

- `reported_comparable_to_dipecs=true` 只允许用于同一个 DiPECS 报告 fixture，或已经在同一 fixture 上复现过的外部模型。
- 所有 Top-K 或 MRR 数值都必须有 `source_url` 和 `source_locator`。
- 百分比指标必须位于 `[0, 100]`。
- 本学术 fixture 中的 `mrr_at_5_pct` 使用百分比；DiPECS 原始报告 JSON 中的 MRR@5 仍保留小数形式。
- 数据集、split 或指标定义不清楚的行必须标为 `excluded_unclear`。
