# 决策路由

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `crates/aios-agent/src/router.rs`, `crates/aios-agent/src/backends/`

**这篇文档回答什么**：一个窗口关闭后，`aios-agent::DecisionRouter` 怎么决定让哪个后端来生成意图。  
**适合谁读**：想理解路由结果、准备新增后端，或者排查“为什么走了 fallback / 为什么没走云端”的人。

## TL;DR

`DecisionRouter` 不依赖单一模型。它按**熔断器 → 隐私敏感度 → 本地可行动信号 → 语义复杂度**的顺序选择后端：

- 默认优先本地、快速、确定性的 `RuleBasedBackend`。
- 出现文件/链接/应用切换等可行动信号时，走 `LocalEvaluatorBackend`。
- 只有云端启用、配置正确且复杂度足够时，才走 `CloudLlmBackend`。
- 云端连续失败触发熔断后，进入 `FallbackNoOpBackend`。

后端的输出只是 `IntentBatch`；动作能否真正执行，还要经过 `PolicyEngine` 按 `CapabilityLevel` 裁定。

## 何时读这篇

| 场景 | 看哪一节 |
| --- | --- |
| 想快速了解四种后端区别 | 后端能力速查表 |
| 想知道某个窗口为什么走了某条路由 | 路由决策树 |
| 要配置或禁用云端 LLM | 配置项速查 |
| 新增/修改后端 | 后端能力速查表 + 关键测试 |
| 排查“为什么 cloud 没生效” | 路由决策树 + 错误处理 |

## 后端能力速查表

| 后端 | 能力 | 风险上限 | 典型延迟 | 是否默认启用 |
| --- | --- | --- | --- | --- |
| `RuleBasedBackend` | `NoOp` / `ReleaseMemory` / `KeepAlive` | `Low` | 微秒级 | 是 |
| `LocalEvaluatorBackend` | 以上 + `PreWarmProcess` / `PrefetchFile` | `Low` | 亚毫秒级 | 是 |
| `CloudLlmBackend` | 全部动作，含 `Medium` 风险 | `Medium` | 百毫秒级～秒级 | 否（需配置） |
| `FallbackNoOpBackend` | 仅 `NoOp` | `Low` | 微秒级 | 熔断时自动 |

所有后端实现同一 trait：

```rust
pub trait DecisionBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult;
    fn evaluate_model_input(&self, input: &ModelInput) -> DecisionBackendResult {
        self.evaluate(&input.current_context)
    }
}
```

`DecisionRouter` 调用 `evaluate_model_input`，因此后端可以选择读取 `ModelMemoryStore` 提供的行为画像和近期反馈。

## 路由决策树

```text
1. 熔断器触发？
   └─ FallbackNoOp
2. 隐私敏感度 > 阈值？
   └─ RuleBased
3. 存在本地可行动信号？
   └─ LocalEvaluator
4. 按语义复杂度选择：
   ├─ 0-1 类 hint ── RuleBased
   ├─ 2-3 类 hint ── CloudLlm（若启用且配置正确）否则 LocalEvaluator
   └─ 4+ 类 hint ── CloudLlm（若启用且配置正确）否则 LocalEvaluator
```

## 输入是什么

`DecisionRouter` 接收的是已经过隐私边界的输入：

- `evaluate`：仅 `StructuredContext`
- `evaluate_model_input`：`ModelInput` = 当前上下文 + 行为画像 + 近期反馈

原始通知文本、文件路径、联系人等 PII 不会进入这里，详见 [模型记忆与行为画像](model-memory.md) 的隐私边界说明。

## 四条路由规则详解

### 1. 熔断器

- 默认 5 次错误 / 60 秒窗口，可通过 `RouterConfig` 调整。
- 只统计 `CloudLlmBackend` 的真实错误（HTTP、JSON、解析失败）。
- `FallbackNoOpBackend` 返回的错误**不**计入熔断，避免永久锁死。
- 云端恢复后，下一次成功调用清零计数。

### 2. 隐私敏感度

以下信号会提高隐私分数：

- `VerificationCode` / `FinancialContext` 等敏感语义 hint
- `AppTransition` 事件

分数超过 `privacy_score_threshold`（默认 3）的窗口强制走 `RuleBased`，避免高隐私价值上下文进入云端或复杂模型。

### 3. 本地可行动信号

满足任一条件即走 `LocalEvaluator`：

- `FileActivity` 事件
- 通知包含 `FileMention` / `ImageMention` / `LinkAttachment`
- `AppTransition::Foreground`

这些信号通常对应明确的本地动作（prefetch、prewarm），不需要云端推理。

### 4. 语义复杂度

按窗口中不同 `SemanticHint` 变体数量分级：

| 唯一 hint 数量 | 路由 |
| --- | --- |
| 0–1 | `RuleBased` |
| 2–3 | `CloudLlm`（若可用），否则 `LocalEvaluator` |
| 4+ | `CloudLlm`（若可用），否则 `LocalEvaluator` |

## 能力边界

`aios-spec::CapabilityLevel::for_route` 给每个路由设定动作白名单和风险上限：

```rust
CapabilityLevel::for_route(DecisionRoute::RuleBased)      // Low, NoOp/ReleaseMemory/KeepAlive
CapabilityLevel::for_route(DecisionRoute::LocalEvaluator) // Low, 全部本地动作
CapabilityLevel::for_route(DecisionRoute::CloudLlm)       // Medium, 全部动作
CapabilityLevel::for_route(DecisionRoute::FallbackNoOp)   // Low, 仅 NoOp
```

这是路由与策略之间的契约：后端可以建议动作，但能否执行由 `PolicyEngine` 裁定。

## 错误处理与降级路径

| 场景 | 行为 |
| --- | --- |
| Cloud HTTP 失败 | 返回 `Idle`/`NoOp` fallback，保留错误，计入熔断 |
| Cloud JSON/翻译失败 | 同上 |
| 熔断触发 | 直接 `FallbackNoOp`，不发起网络请求 |
| 本地后端 | 确定性计算，理论上不出错 |

`DecisionRouter` 发现 `CloudLlmBackend` 返回错误后，会再用 `RuleBasedBackend` 决策一次，确保窗口仍有可用结果：

```text
CloudLlm -> error
  -> RuleBased fallback
    -> PolicyEngine / ActionLifecycle
```

## 配置项速查

```rust
RouterConfig {
    privacy_score_threshold: 3,
    circuit_breaker_threshold: 5,
    circuit_breaker_window_secs: 60,
}
```

`CloudLlmBackend` 是否可用由 `CloudBackendState::from_env()` 决定，关键变量见 `DIPECS_CLOUD_LLM_*` 系列，详情参考 [云端 LLM 后端](cloud-llm.md)。

## 关键测试

- `crates/aios-agent/src/router.rs`
  - `circuit_state_persists_across_evaluate_calls`
  - `circuit_state_resets_after_successful_fallback`
  - `circuit_state_counts_cloud_backend_errors`
  - `cloud_disabled_medium_complexity_routes_to_local_evaluator`
- `crates/aios-agent/tests/mock_cloud_proxy_test.rs`
  - 覆盖文件提及、应用切换、隐私降级、熔断触发、fallback 审计可见性
- `crates/aios-agent/tests/baseline_comparison_test.rs`
  - `baseline_privacy_and_governance_comparison` 验证模型输入无 raw text 泄漏

## 相关文档

- [模型记忆与行为画像](model-memory.md)
- [云端 LLM 后端](cloud-llm.md)
- [管线与运行时](pipeline.md)
- [状态机](states.md)
- [RFC-0001 分层采集与决策路由重构](../rfc/0001-layered-collection-and-decision-routing.md)
