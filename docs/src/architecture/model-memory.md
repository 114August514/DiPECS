# 模型记忆与行为画像

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `crates/aios-core/src/context_memory.rs`, `crates/aios-spec/src/context.rs`, `crates/aios-daemon/src/pipeline.rs`

**这篇文档回答什么**：系统如何在不回传原始事件的前提下，把历史窗口信息用于后续决策。  
**适合谁读**：想理解行为画像来源、确认隐私边界，或者调整记忆参数的人。

## TL;DR

`ModelMemoryStore` 在每个窗口关闭后，基于**已脱敏**的 `StructuredContext`、决策结果和审计记录，滚动维护：

- `UserBehaviorProfile`：用户高频应用、语义 hint、动作成败的统计画像。
- `RecentDecisionRecord`：最近若干窗口的决策与执行反馈。

这些记忆通过 `ModelInput` 回注到下一窗口的决策输入。原始通知文本、文件路径、联系人等 PII 永远不会进入记忆。

## 何时读这篇

| 场景 | 看哪一节 |
| --- | --- |
| 想快速了解记忆里保存了什么 | 记忆里有什么 |
| 要调整窗口数量、top-k、衰减系数 | 配置表 |
| 担心 PII 是否会泄漏到模型输入 | 隐私保证 |
| 想理解 `PreWarm` 效果如何归因 | PreWarm 命中/缺失归因 |
| 想知道后台 LLM 摘要怎么工作 | 后台 ProfileSummarizer |

## 为什么需要模型记忆

如果每个窗口都从零决策，系统会反复问“这个用户在做什么”。`ModelMemoryStore` 让后端看到：

- 用户近期常出现在前台的 app
- 近期频繁出现的语义 hint
- 过去建议动作的执行结果

同时约束条件是：**不能保存或回传原始事件**。所有输入必须经过 `PrivacyAirGap` 脱敏。

## 记忆里有什么

### `UserBehaviorProfile`

| 字段 | 含义 |
| --- | --- |
| `summary` | 本地计数文本；若启用了 LLM 摘要则前缀合并 |
| `observation_windows` | 已观察窗口数 |
| `frequent_foreground_apps` | 高频前台应用及分数 |
| `frequent_notifying_apps` | 高频通知应用及分数 |
| `frequent_semantic_hints` | 高频语义 hint 及分数 |
| `action_successes` | 成功动作及次数 |
| `action_denials` | 被拒绝动作及次数 |
| `action_failures` | 执行失败动作及次数 |
| `last_updated_window_id` | 最后更新窗口 ID |

分数优先使用带衰减的动量分；动量不足时回退到原始计数。

### `RecentDecisionRecord`

每个关闭窗口产生一条记录：

| 字段 | 含义 |
| --- | --- |
| `window_id` / 起止时间 | 窗口定位 |
| `foreground_apps` / `notified_apps` | 该窗口的 app 集合 |
| `semantic_hints` | 该窗口的语义 hint 集合 |
| `route` / `model` | 决策路由与模型 |
| `intent_count` | 意图数量 |
| `rationale_tags` | 决策理由标签 |
| `backend_error` | 后端错误（如有） |
| `action_outcomes` | 每个动作的反馈结果 |

## 记忆如何更新

`crates/aios-daemon/src/pipeline.rs::process_window` 中的顺序：

```text
1. model_input(ctx)          # 构造当前输入
2. router.evaluate_model_input(input)
3. ActionLifecycle.run(...)  # 治理 + 审计
4. observe_window(...)       # 把结果写回记忆
5. ProfileSummaryWorker.poll / maybe_start
6. persist_if_configured(...) # 若配置路径则持久化
```

## 配置表

| 字段 | 默认值 | 环境变量 | 作用 |
| --- | --- | --- | --- |
| `recent_limit` | 5 | `DIPECS_MODEL_MEMORY_RECENT_WINDOWS` | 保留近期窗口数 |
| `top_limit` | 8 | `DIPECS_MODEL_MEMORY_TOP_K` | 画像 top-k 项 |
| `momentum_decay_milli` | 900 | `DIPECS_MODEL_MEMORY_MOMENTUM_DECAY_MILLI` | 每窗口动量衰减系数 |
| `prewarm_effect_windows` | 3 | `DIPECS_PREWARM_EFFECT_WINDOWS` | PreWarm 归因窗口数 |
| `persist_path` | `None` | `DIPECS_MODEL_MEMORY_PATH` | 持久化文件路径 |

`momentum_decay_milli / 1000` 让近期窗口权重更高。

## PreWarm 命中/缺失归因

`PreWarmProcess` 动作成功后，系统会记录一个 `PendingPrewarm`：

- 如果在 `prewarm_effect_windows` 个窗口内目标应用出现在前台，
  原始 `RecentDecisionRecord` 的反馈更新为 `PredictionHit`。
- 否则更新为 `PredictionMiss`。

这让后续分析可以判断“提前预热是否真的命中”。

## 后台 `ProfileSummarizer`

`crates/aios-agent/src/backends/cloud_llm/summarizer.rs`

- 把 `UserBehaviorProfile` 和 `recent_feedback` 发给云端，请求生成 80 词以内纯文本摘要。
- 启用条件：云端 LLM 已配置 **且** `DIPECS_PROFILE_SUMMARY_ENABLED=true`。
- 在后台线程运行，不阻塞主窗口处理。

## 隐私保证

`ModelMemoryStore` 只保存已脱敏信息：

- app 包名（不含通知内容）
- `SemanticHint` 类型（不含原始文本）
- 动作类型与结果（不含原始 target 细节）
- 决策路由、模型、rationale tags

以下信息**不会**进入 `ModelInput`：

- 原始通知文本
- 文件路径、URL 原文
- 联系人、短信内容

相关守护测试：

- `crates/aios-core/tests/privacy_leak_test.rs`
- `crates/aios-core/tests/privacy_airgap_property_test.rs`
- `crates/aios-core/tests/privacy_airgap_test.rs`
- `crates/aios-agent/tests/baseline_comparison_test.rs`

## 如何查看与调试

- 在 `dipecsd` trace 中观察 `behavior_profile` 和 `recent_feedback` 字段。
- 设置 `DIPECS_MODEL_MEMORY_PATH` 后，每个窗口结束会写出 JSON，可直接查看。
- 调整 `DIPECS_MODEL_MEMORY_RECENT_WINDOWS` 可控制近期窗口数量。

## 相关文档

- [决策路由](decision-routing.md)
- [云端 LLM 后端](cloud-llm.md)
- [管线与运行时](pipeline.md)
- [隐私边界与 Android 安全](../android/security-privacy.md)
