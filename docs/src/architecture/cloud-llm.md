# 云端 LLM 后端

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `crates/aios-agent/src/backends/cloud_llm/`

**这篇文档回答什么**：`CloudLlmBackend` 如何接入 OpenAI-compatible 云端模型，以及它为什么默认不参与、失败后系统如何自保。  
**适合谁读**：要启用/配置云端 LLM、理解延迟，或解读云端基准结果的人。

## TL;DR

`CloudLlmBackend` 是 DiPECS 能力最高、延迟也最高的**可选**后端：

- 默认关闭，必须显式配置 `DIPECS_CLOUD_LLM_*` 环境变量才会启用。
- 支持 DeepSeek、Qwen 和任意 OpenAI-compatible 端点。
- 任何 HTTP、JSON、翻译失败都会返回 `Idle`/`NoOp` fallback，并触发路由层降级。
- 云端失败会累计到熔断器，熔断后系统进入 `FallbackNoOpBackend`。

## 何时读这篇

| 场景 | 看哪一节 |
| --- | --- |
| 想启用云端 LLM | 启用条件 checklist |
| 想知道支持哪些 provider | Provider 矩阵 |
| 想理解模型该返回什么 JSON | 模型应返回的 JSON schema |
| 遇到 cloud 失败 / fallback | 失败安全机制 |
| 想跑延迟/场景基准 | 如何运行基准 |

## 启用条件 checklist

必须同时满足：

- [ ] `DIPECS_CLOUD_LLM_ENABLED=true`
- [ ] `DIPECS_CLOUD_LLM_PROVIDER` 已设置（`deepseek` / `qwen` / `generic`）
- [ ] `DIPECS_CLOUD_LLM_ENDPOINT` 已设置（或使用 provider 默认值）
- [ ] `DIPECS_CLOUD_LLM_MODEL` 已设置（或使用 provider 默认值）
- [ ] `DIPECS_CLOUD_LLM_API_KEY` 已设置（或 `DEEPSEEK_API_KEY` / `DASHSCOPE_API_KEY`）

可选：

- `DIPECS_CLOUD_LLM_TIMEOUT_SECS`（默认 15）
- `DIPECS_CLOUD_LLM_TEMPERATURE`（默认 0.1）
- `DIPECS_CLOUD_LLM_SYSTEM_PROMPT`
- `DIPECS_CLOUD_LLM_ENABLE_THINKING`
- `DIPECS_CLOUD_LLM_REASONING_EFFORT`

## Provider 矩阵

| Provider | 默认 endpoint | 默认 model | 特殊请求字段 |
| --- | --- | --- | --- |
| DeepSeek | `https://api.deepseek.com/chat/completions` | `deepseek-v4-flash` | `thinking: { type: ... }` |
| Qwen | `https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions` | `qwen-plus` | `enable_thinking: bool` |
| Generic | 用户指定 | 用户指定 | 无 |

## 请求到响应的完整流程

```text
ModelInput
  -> render_model_input_prompt (JSON 序列化)
  -> build_request_body (system + user prompt)
  -> POST /chat/completions
  -> 解析响应，去掉 markdown fence
  -> translate.rs 归一化
  -> IntentBatch
```

用户 prompt 包含当前上下文、行为画像、近期反馈的 JSON 序列化。

## 模型应返回的 JSON schema

```json
{
  "intents": [
    {
      "intent_type": "SwitchToApp",
      "target": "com.example.reader",
      "confidence": 0.92,
      "risk_level": "Low",
      "actions": [
        { "action_type": "PreWarmProcess", "target": "com.example.reader", "urgency": "Normal" }
      ],
      "rationale_tags": ["foreground_transition"]
    }
  ]
}
```

支持的 `intent_type`：`OpenApp`、`SwitchToApp`、`CheckNotification`、`HandleFile`、`EnterContext`、`Idle`。

## 翻译层做了什么

`crates/aios-agent/src/backends/cloud_llm/translate.rs`：

- 去掉 markdown 代码块围栏。
- 校验 JSON 结构。
- 对 `intent_type`、`action_type`、`risk_level`、`urgency`、`extension_category` 做大小写不敏感归一化。
- 把 `confidence` 钳制到 `[0.0, 1.0]`。
- 解析 `PrefetchFile` 的 target（支持 `url:`、`uri:`、`pkg:` 前缀，并按类别提供默认 URL）。
- 若模型无输出或解析失败，注入 `Idle`/`NoOp` fallback。

## 失败安全机制

| 失败类型 | 行为 |
| --- | --- |
| HTTP 错误 | 返回 `Idle`/`NoOp` + `cloud_llm_error` rationale |
| JSON 解析失败 | 同上 |
| 字段归一化失败 | 同上 |
| 熔断触发 | 直接 `FallbackNoOp`，不发起请求 |

路由层捕获 cloud error 后，会用 `RuleBasedBackend` 再决策一次，确保窗口仍有结果。

## 后台 `ProfileSummarizer`

- 复用同一套 `CloudLlmConfig`。
- 把 `UserBehaviorProfile` 和 `recent_feedback` 发给云端，请求生成 80 词以内摘要。
- 启用条件：`DIPECS_PROFILE_SUMMARY_ENABLED=true`。
- 后台线程运行，不阻塞主窗口。

详情见 [模型记忆与行为画像](model-memory.md)。

## 如何运行基准

以下测试需要真实 API key，默认被 `#[ignore]`：

```bash
# 4 个场景各跑一次
cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::smoke -- --ignored --nocapture

# 10 轮 DeepSeek 延迟基准
cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::latency -- --ignored --nocapture
```

输出：

- `data/evaluation/cloud-latency-<ts>.json`
- `data/evaluation/cloud-scenarios-<ts>.json`

## 已有评估数据快照

- `data/evaluation/cloud-latency-20260701-084110.json`
  - DeepSeek `deepseek-v4-flash`
  - 5 轮 `morning-routine`
  - p50 ≈ 11.3 s，p95 ≈ 13.0 s，成功率 100%
- `data/evaluation/cloud-scenarios-20260701-084010.json`
  - 4 个场景全部成功

## 关键设计点

- **OpenAI-compatible**：provider 差异仅体现在 endpoint/model 默认值和 `thinking`/`enable_thinking` 字段。
- **Latency-tracked**：每次调用记录 `latency_us`。
- **Fail-safe**：任何错误都转成 `Idle`/`NoOp`。
- **非默认启用**：必须显式配置才会参与路由。

## 相关文档

- [决策路由](decision-routing.md)
- [模型记忆与行为画像](model-memory.md)
- [管线与运行时](pipeline.md)
- [模拟器评估套件](../evaluation/emulator-evaluation-suite.md)
