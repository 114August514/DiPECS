# DiPECS Baseline 框架

本页索引 DiPECS 全项目的 baseline（对照组）体系，说明每个维度的“对照组是什么、如何运行、如何解读”。

Baseline 是判断一个优化是否有效的前提：没有对照组，任何绝对数字都难以说明价值。

## 维度总览

| 维度 | 已有 Baseline | 关键文件/工具 |
| --- | --- | --- |
| **隐私与治理** | naive cloud prompt vs DiPECS pipeline | `crates/aios-agent/tests/baseline_comparison_test.rs` |
| **资源开销** | `baseline_idle` / `dipecs_observe_only` / `dipecs_action_loop` | `tools/collect-resource-overhead.sh`, `crates/aios-cli/tests/resource_overhead_dataset_test.rs` |
| **UX 体验** | `no_dipecs_baseline` / `cold_startup` / `prewarm_startup` | `tools/collect-ux-metrics.sh`, `crates/aios-cli/tests/ux_metrics_dataset_test.rs` |
| **稳定性** | 长时间运行内存泄漏基线 | `tools/collect-stability.sh`, `crates/aios-cli/tests/stability_dataset_test.rs` |
| **云端决策延迟** | RuleBased/LocalEvaluator vs CloudLLM(DeepSeek) | `crates/aios-agent/src/backends/cloud_llm/mod.rs` latency/cloud_bench tests |
| **动作执行覆盖** | mock-socket / emulator action-loop | `crates/aios-action/tests/android_bridge_e2e_test.rs`, `tests/scenarios/action-loop-e2e.sh` |
| **next-app 预测** | 随机/首个候选/全局多数/条件多数/Markov/总是 NoOp | `aios-cli benchmark-next-app`, `crates/aios-cli/tests/benchmark_next_app_test.rs` |
| **policy denial 率** | 默认 PolicyEngine vs 策略大门完全敞开 | `tests/integration/policy_denial.rs` |
| **routing strategy** | 固定路由 vs DecisionRouter 动态路由 | `tests/integration/routing_strategy.rs` |
| **noop 覆盖率** | RuleBased/LocalEvaluator vs 总是 NoOp | `tests/integration/noop_coverage.rs` |
| **窗口大小资源/吞吐** | 1s / 10s / 60s 窗口 replay 性能 | `tests/integration/window_size.rs` |
| **CloudLLM 稳定性** | 多次调用输出变化率 / JSON 解析失败率 | `tests/integration/cloud_llm_stability.rs` |
| **动作成功率** | 四类动作 mock bridge 成功/失败分布 | `tests/integration/action_success_rate.rs` |
| **HMAC 签名交叉验证** | 标准库 `hmac` + `sha2` 独立重算 | `tests/integration/signature_cross_verify.rs` |
| **rationale tags 覆盖率** | RuleBased/LocalEvaluator vs 统计基线 | `tests/integration/rationale_coverage.rs` |

所有新增 baseline 统一通过根目录 integration test crate 运行：

```bash
cargo test --test integration
```

运行单个 baseline：

```bash
cargo test --test integration policy_denial
cargo test --test integration routing_strategy
cargo test --test integration noop_coverage
cargo test --test integration window_size
cargo test --test integration action_success_rate
cargo test --test integration signature_cross_verify
cargo test --test integration rationale_coverage
```

CloudLLM 稳定性测试默认 `#[ignore]`，需要真实 API key：

```bash
DIPECS_CLOUD_LLM_API_KEY=xxx cargo test --test integration cloud_llm_stability -- --ignored --nocapture
```

## 1. 隐私与治理

**对照组**：把包含 raw_title/raw_text 的原始通知 JSON 直接发给云端 LLM，让模型决定动作。

**实验组**：同样的 trace 经过 DiPECS 的 `PrivacyAirGap` + `DecisionRouter` + `PolicyEngine`，看 model input / audit 中是否还有 raw text，以及哪些动作被策略拦截。

**运行**：

```bash
cargo test -p aios-agent --test baseline_comparison_test
```

**解读**：

- naive prompt 中应包含若干 raw notification text（否则对照组无意义）。
- DiPECS pipeline 的 model input / audit 中必须 0 泄漏。
- 同时观察 `DeniedByCapability` / `TargetNotInContext` 等治理事件。

## 2. 资源开销

**对照组**：`baseline_idle`（app force-stop，系统基线）。

**实验组**：

- `dipecs_observe_only`：仅采集，不动作。
- `dipecs_action_loop`：采集 + 持续发送 KeepAlive / ReleaseMemory / PreWarm / Prefetch。

**运行**：

```bash
./tools/collect-resource-overhead.sh
```

**解读**：

- 关注 CPU Δ、PSS Δ、RSS Δ。
- 当前阈值：CPU Δ ≤ 8 pp，PSS Δ ≤ 80 MB。

## 3. UX 体验

**对照组**：`no_dipecs_baseline` / `cold_startup`（无 DiPECS 的冷启动）。

**实验组**：`prewarm_startup`（DiPECS 预热后再启动 MainActivity）、`post_release_jank`（ReleaseMemory 后帧率）。

**运行**：

```bash
./tools/collect-ux-metrics.sh
```

**解读**：

- PreWarm 启动加速 ≥ 20% 或 ≥ 100 ms 视为有效。
- ReleaseMemory 不应使 jank 增加超过 20 个百分点。

## 4. 稳定性

**对照组**：无（自身前后对比）。通过长时间采样 RSS/PSS/CPU，用线性回归判断增长速率。

**运行**：

```bash
DURATION_MINUTES=60 ./tools/collect-stability.sh
```

**解读**：

- RSS 增长 ≤ 50 MB/h，PSS 增长 ≤ 20 MB/h，平均 CPU ≤ 10%。

## 5. 云端决策延迟

**对照组**：本地 `RuleBasedBackend` / `LocalEvaluatorBackend`（亚毫秒级）。

**实验组**：`CloudLlmBackend` 调用真实 DeepSeek API。

**运行**：

```bash
source .env
cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::latency -- --ignored --nocapture
```

**解读**：

- 本地后端 p50 < 0.1 ms，云端 p50 约 6–11 s。
- 支撑“本地优先、云端仅兜复杂语义”的路由策略。

## 6. 动作执行覆盖

**对照组**：mock-socket 本地接住 bridge payload，验证签名、HMAC、action_type JSON 值与 Debug 值一致。

**实验组**：emulator / 真机上完整动作回路（daemon → signed payload → Android app → handler → audit）。

**运行**：

```bash
cargo test -p aios-action --test android_bridge_e2e_test
bash tests/scenarios/action-loop-e2e.sh
```

**解读**：

- mock 测试保证 Rust 侧转发逻辑正确。
- 真机/模拟器测试验证四类可转发动作（KeepAlive/ReleaseMemory/PreWarmProcess/PrefetchFile）确实 EXECUTED。

## 7. next-app 预测

**对照组**：

- `random_candidate`：从可观测候选中随机选一个。
- `first_candidate`：取候选列表第一个。
- `global_majority`：总是预测训练集里最常出现的下一应用。
- `per_current_app_majority`：按当前应用，预测历史上最常接在它后面的应用。
- `markov`：按 `P(next_app | current_app)` 排序候选。
- `always_noop`：总是不预测，对应 100% NoOp 率。

**实验组**：`RuleBasedBackend`、`LocalEvaluatorBackend`。

**运行**：

```bash
cargo run --bin aios-cli -- benchmark-next-app \
  --input data/traces/scenarios/morning-routine.jsonl \
  --input data/traces/scenarios/multi-app-switching.jsonl \
  --input data/traces/scenarios/rich-workflow.jsonl \
  --labels data/traces/synthetic-next-app-v1.labels.jsonl \
  --output data/evaluation/synthetic-next-app-v1.report.json \
  --train-split 0.7 \
  --window-secs 10
```

**解读**：

- 只在 `eligible` 样本上评估（即真实下一应用已在当前上下文中可观测）。
- 统计基线只在训练 split 上拟合，测试 split 上评估。
- 当前合成数据上，随机候选 Top-1 约 62%，Markov 可达 65–75%。RuleBased ~61% 仅相当于随机水平。
- 报告 schema 为 `dipecs.next_app_benchmark.v2`；若消费端校验 schema version，需同步升级。

## 8. policy denial 率

**对照组**：策略大门完全敞开（`PolicyConfig { max_auto_risk: High, ..Default }`），模拟无 `PolicyEngine` 拦截时，同一条 High risk / 未知 target 动作会到达执行器并成功执行。

**实验组**：生产默认 `PolicyEngine` + `CapabilityLevel::for_route(CloudLlm / LocalEvaluator)`，High risk / 未知 target 的动作被拒绝。

**运行**：

```bash
cargo test --test integration policy_denial
```

**解读**：

- 默认策略下，CloudLLM 与 LocalEvaluator 能力档位的 `max_risk` 均低于 `High`，越权动作 100% 被拦截。
- 策略大门敞开时，同一条动作 100% 到达执行器。
- 证明 `PolicyEngine` 是防止后端越权动作的最后一道防线。

## 9. routing strategy

**对照组**：固定 `RuleBasedBackend` / `LocalEvaluatorBackend` 单独评估。

**实验组**：生产默认 `DecisionRouter` 根据隐私分和语义复杂度动态选择后端。

**运行**：

```bash
cargo test --test integration routing_strategy
```

**解读**：

- 高隐私分 trace（如 500 次 AppTransition）回退到 `RuleBased`，与固定 RuleBased 等价。
- 低隐私分但富语义信号（FileMention / ImageMention / LinkAttachment）升级到 `LocalEvaluator`，优于固定 RuleBased。
- 证明动态路由不劣于任何固定路由。

## 10. noop 覆盖率

**对照组**：`always_noop`（100% NoOp、0% 预测覆盖）。

**实验组**：`RuleBasedBackend`、`LocalEvaluatorBackend`。

**运行**：

```bash
cargo test --test integration noop_coverage
```

**解读**：

- `RuleBased` 与 `LocalEvaluator` 的 NoOp 率必须显著低于 100%，且预测覆盖率明显高于 0%。
- 当前阈值（按 synthetic-next-app-v1 实测校准）：
  - aggregate：`rule_based` NoOp ≤ 25%、覆盖率 ≥ 55%；`local_evaluator` NoOp ≤ 45%、覆盖率 ≥ 50%。
  - per scenario：`rule_based` NoOp ≤ 35%、覆盖率 ≥ 50%；`local_evaluator` NoOp ≤ 55%、覆盖率 ≥ 35%。
- realistic prior 对比：`markov` / `per_current_app_majority` 覆盖率为 100%，DiPECS 覆盖率不得低于其 45pp 以上；DiPECS NoOp 率必须 < 50%，远离 trivial `always_noop`。
- 若真实后端接近 `always_noop`，说明其未产生有效动作建议。

## 11. 窗口大小资源/吞吐

**对照组**：1s 窗口（窗口管理开销最大）。

**实验组**：10s / 60s 窗口。

**运行**：

```bash
cargo test --test integration window_size -- --nocapture
```

**解读**：

- 更大窗口的吞吐（events/ms）不应灾难性下降（10s ≥ 1s 的 85%，60s ≥ 1s 的 65%）。
- 更大窗口的峰值 RSS 与 CPU 时间不应超过 1s 窗口的 1.5 倍。
- 本机实测值（debug 模式）：1s 约 9.8–11.7 ev/ms、peak RSS ≈ 14.4 MiB、cpu_total ≈ 120–240 ms；10s 与 60s 均满足上述收紧后的阈值，无需进一步校准。
- 帮助权衡实时性（小窗口）与批处理效率（大窗口）。

## 12. CloudLLM 稳定性

**对照组**：同一输入调用一次。

**实验组**：同一输入重复调用 N 次（默认 10，可通过 `CLOUD_BENCH_ROUNDS` 覆盖）。

**运行**：

```bash
DIPECS_CLOUD_LLM_API_KEY=xxx cargo test --test integration cloud_llm_stability -- --ignored --nocapture
```

**解读**：

- 统计成功返回 `IntentBatch` 的次数、JSON 解析失败次数、相邻调用间 intent 集合变化率。
- 用于量化云端后方的输出方差，支撑“云端仅作兜底、本地优先”的决策。

## 13. 动作成功率

**对照组**：mock bridge 本地接住 payload，模拟设备返回 `ok` / `rejected`。

**实验组**：`ActionLifecycle` + `AndroidBridgeAdapter` 驱动四类动作，观察 terminal state。

**运行**：

```bash
cargo test --test integration action_success_rate
```

**解读**：

- PreWarmProcess、KeepAlive、ReleaseMemory、PrefetchFile(url:) 在设备 `ok` 时都应 `Succeeded`。
- 同一批动作在设备 `rejected` 时都应 `Failed`。
- NoOp 与 PrefetchFile(pkg:) 走本地 stub，不经过 bridge 也 `Succeeded`。
- 给出 per-action-type 的 forwarded / local-stub / rejected 分布。

## 14. HMAC 签名交叉验证

**对照组**：无（自洽验证）。

**实验组**：用独立的标准库 `hmac` + `sha2` 重新计算 `AuthorizedAction` payload 的 HMAC signature，与生产代码生成的 signature 比较。

**运行**：

```bash
cargo test --test integration signature_cross_verify
```

**解读**：

- 生产签名实现与标准库实现必须逐字节一致。
- 验证 token 敏感（换 token 则 signature 变）、长度前缀防止拼接歧义。

## 15. rationale tags 覆盖率

**对照组**：统计基线（random / first / global_majority / per_current_app_majority / markov / always_noop）不产出 DiPECS intents，rationale 覆盖率应为 0.0%。

**实验组**：`RuleBasedBackend`、`LocalEvaluatorBackend`。

**运行**：

```bash
cargo test --test integration rationale_coverage
```

**解读**：

- DiPECS 后端产出的 intent 中，至少有一个 intent 带有非空 `rationale_tags` 的窗口比例：aggregate ≥ 95%，per scenario ≥ 90%。
- 统计基线的 rationale 覆盖率必须为 0.0%。
- 保证可解释性标签是 DiPECS 后端的固有属性，而非 benchmark artifact。

## 相关文档

- [评估工具](tools.md)
- [模拟器评估套件](emulator-evaluation-suite.md)
- [RFC-0002 Action Bus 治理](docs/src/rfc/0002-action-bus-governance.md)
