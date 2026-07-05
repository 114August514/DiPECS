# DiPECS Baseline 框架

本页索引 DiPECS 全项目的 baseline（对照组）体系，说明每个维度的“对照组是什么、如何运行、如何解读”。

Baseline 是判断一个优化是否有效的前提：没有对照组，任何绝对数字都难以说明价值。

## 维度总览

| 维度 | 已有 Baseline | 关键文件/工具 |
| --- | --- | --- |
| **隐私与治理** | naive cloud prompt vs DiPECS pipeline | `crates/aios-agent/tests/baseline_comparison_test.rs` |
| **资源开销** | `baseline_idle` / `dipecs_observe_only` / `dipecs_action_loop` | `tools/collect/collect-resource-overhead.sh`, `crates/aios-cli/tests/resource_overhead_dataset_test.rs` |
| **UX 体验** | `no_dipecs_baseline` / `cold_startup` / `prewarm_startup` | `tools/collect/collect-ux-metrics.sh`, `crates/aios-cli/tests/ux_metrics_dataset_test.rs` |
| **稳定性** | 长时间运行内存泄漏基线 | `tools/collect/collect-stability.sh`, `crates/aios-cli/tests/stability_dataset_test.rs` |
| **云端决策延迟** | RuleBased/LocalEvaluator vs CloudLLM(DeepSeek) | `crates/aios-agent/src/backends/cloud_llm/mod.rs` latency/cloud_bench tests |
| **动作执行覆盖** | mock-socket / emulator action-loop | `crates/aios-action/tests/android_bridge_e2e_test.rs`, `tests/scenarios/action-loop-e2e.sh` |
| **next-app 预测** | 平凡(随机/首个/NoOp) + 统计(全局/条件多数/Markov) + realistic prior(最近通知/通知优先级/最近前台/最近预热) | `aios-cli benchmark-next-app`, `crates/aios-cli/tests/benchmark_next_app_test.rs` |
| **policy denial 率** | 默认 PolicyEngine vs 策略大门完全敞开 | `tests/integration/policy_denial.rs` |
| **routing strategy** | 固定路由 + hardcoded_routing 简单路由 vs DecisionRouter 动态路由；含 cloud-avoidance 率 | `tests/integration/routing_strategy.rs` |
| **noop 覆盖率** | RuleBased/LocalEvaluator vs 总是 NoOp + realistic prior | `tests/integration/noop_coverage.rs` |
| **窗口大小资源/吞吐** | 1s / 10s / 60s 窗口 replay 性能 | `tests/integration/window_size.rs` |
| **CloudLLM 稳定性** | 确定性 RuleBased/LocalEvaluator 对照组 vs 云端多次调用变化率 | `tests/integration/cloud_llm_stability.rs` |
| **动作成功率** | 直接转发(前 DiPECS) + 四类动作 mock bridge 成功/失败分布 | `tests/integration/action_success_rate.rs` |
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
./tools/collect/collect-resource-overhead.sh
```

**解读**：

- 关注 CPU Δ、PSS Δ、RSS Δ。
- 当前阈值：CPU Δ ≤ 8 pp，PSS Δ ≤ 80 MB。

## 3. UX 体验

**对照组**：`no_dipecs_baseline` / `cold_startup`（无 DiPECS 的冷启动）。

**实验组**：`prewarm_startup`（DiPECS 预热后再启动 MainActivity）、`post_release_jank`（ReleaseMemory 后帧率）。

**运行**：

```bash
./tools/collect/collect-ux-metrics.sh
```

**解读**：

- PreWarm 启动加速 ≥ 20% 或 ≥ 100 ms 视为有效。
- 启动对比至少 cold/prewarm 合计 n≥20，并报告均值 + p95。
- ReleaseMemory 不应使 jank 增加超过 20 个百分点。

## 4. 稳定性

**对照组**：无（自身前后对比）。通过长时间采样 RSS/PSS/CPU，用线性回归判断增长速率。

**运行**：

```bash
DURATION_MINUTES=60 ./tools/collect/collect-stability.sh
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

平凡对照组（trivial control）——不利用真实上下文信号，仅用于下界参考：

- `random_candidate`：从可观测候选中随机选一个。
- `first_candidate`：取候选列表第一个。
- `always_noop`：总是不预测，对应 100% NoOp 率。

统计对照组（statistical prior）——在训练 split 上拟合频率：

- `global_majority`：总是预测训练集里最常出现的下一应用。
- `per_current_app_majority`：按当前应用，预测历史上最常接在它后面的应用。
- `markov`：按 `P(next_app | current_app)` 排序候选。

realistic prior（前 DiPECS 的启发式对照组）——模拟真机 launcher / 系统在没有 DiPECS
时可用的常见启发式，只读取窗口内可观测信号，确定性、无 RNG：

- `recent_notification`：最近一条通知的来源应用。对应“最近通知的应用很可能被打开”。
- `notification_priority`：按通知优先级打分（is_ongoing / FileMention / ImageMention /
  LinkAttachment / UserMentioned / CalendarInvitation / alarm·call·event 类别 / 最近时间戳），
  取分最高候选。对应 launcher 的通知重要度排序。
- `last_foreground`：最近一次非当前应用的前台切换目标。对应“切回上一个应用”。
- `last_app_prewarm`：最近被切换到（可视为被预热）的应用；因合成 trace 无显式预热历史，
  暂用最近前台切换目标作代理，实现与 `last_foreground` 一致但语义命名不同。

> 之所以引入 realistic prior，是因为只跟 `always_noop` / `random_candidate` 比不能证明
> DiPECS 的价值：如果前人的简单启发式已经 cover 了大多数场景，那 DiPECS 就没有意义。
> 只有跟真实启发式对比、并保持可解释性/隐私/治理优势，价值才成立。

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
- 当前合成数据上的 aggregate Top-1 / 覆盖率 / NoOp / rationale（见
  `data/evaluation/synthetic-next-app-v1.report.json`）：
  - `rule_based`：Top-1 61.8%、覆盖 61.8%、NoOp 16.4%、rationale 100%。
  - `local_evaluator`：Top-1 49.1%、覆盖 56.4%、NoOp 36.4%、rationale 100%。
  - realistic prior：`recent_notification` 65.5% / `notification_priority` 61.8% /
    `last_foreground`·`last_app_prewarm` 23.6%。
  - 统计 prior：`markov`·`per_current_app_majority` 83.6%、`global_majority` 85.5%。
- 注意：合成数据候选集很小（1–2 个），使 `random_candidate` Top-1 高达 83.6%——这正是
  不能用它作主对照组的原因。realistic prior 中 `recent_notification` 已略高于 `rule_based`，
  说明在合成数据上单纯的 Top-1 并不能体现 DiPECS 价值；DiPECS 的真正优势在于 100% rationale
  覆盖、隐私气隙与治理拦截（见 §1、§8、§15），而非纯预测准确率。
- 报告 schema 为 `dipecs.next_app_benchmark.v2`；若消费端校验 schema version，需同步升级。
  报告由代码生成 `evaluation_source`、`action_value_source` 和 `benefit_claim_policy`：
  当前值表示合成 trace 的 prediction-only 回归，不导入真实 action-value 测量，也不输出
  `net_benefit_ms`。因此它只能用于快速回归预测到动作候选的映射，不能用于系统收益主张。

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

**对照组**：

- 固定 `RuleBasedBackend` / `LocalEvaluatorBackend` 单独评估。
- `hardcoded_routing`：模拟前人系统的简单静态路由（`privacy_score > 3 => RuleBased，否则
  LocalEvaluator`，无熔断、无按复杂度上云）。其 `compute_privacy_score` 与生产
  `DecisionRouter` 逐行一致（生产函数 crate-private，故在测试内忠实复刻）。

**实验组**：生产默认 `DecisionRouter` 根据隐私分和语义复杂度动态选择后端。

**运行**：

```bash
cargo test --test integration routing_strategy
```

**解读**：

- 高隐私分 trace（如 500 次 AppTransition）回退到 `RuleBased`，与固定 RuleBased 等价。
- 低隐私分但富语义信号（FileMention / ImageMention / LinkAttachment）升级到 `LocalEvaluator`，优于固定 RuleBased。
- 证明动态路由不劣于任何固定路由，也不劣于 `hardcoded_routing` 简单路由。
- 为防止复刻的 `compute_privacy_score` 与生产悄悄偏离，测试解析生产路由发出的
  `routing:privacy_sensitive(score=N)` rationale tag，断言解析出的分值与复刻实现相等。
- **cloud-avoidance 率**（性能价值链接）：把三条 scenario trace 按真实窗口切分（`build_windows`
  复刻 daemon 的窗口关闭逻辑），逐窗口过 `DecisionRouter::default()`，统计路由到本地
  （RuleBased / LocalEvaluator）而非 CloudLlm / FallbackNoOp 的比例，断言 ≥ 80%。
  由于云端 p50 约 6–11 s、本地亚毫秒，避免上云是主要的用户可见延迟收益。
  诚实说明：未配置云端 key 时 `cloud_route_or_fallback` 永不返回 CloudLlm、熔断也不触发，
  该比例结构上恒为 100%，`≥ 80%` 只有在配置云端 key 后才可能被证伪；此测试当前价值在于
  验证默认路由在真实逐窗口分布下始终留在本地，并锻炼 `build_windows`。

## 10. noop 覆盖率

**对照组**：

- `always_noop`（100% NoOp、0% 预测覆盖）——trivial 下界。
- 高覆盖统计 prior（`markov` / `per_current_app_majority`，覆盖率 100%）——覆盖率上界参照。

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

**对照组**：确定性本地后端 `RuleBasedBackend` / `LocalEvaluatorBackend`。同一 `ModelInput`
调用 10 次，intent 集合变化率与 JSON 失败率均应为 0%（这是"完美稳定"的参照下界）。

**实验组**：`CloudLlmBackend` 对同一输入重复调用 N 次（默认 10，可通过 `CLOUD_BENCH_ROUNDS` 覆盖）。

**运行**：

```bash
# 确定性对照组（无需 key，默认运行）
cargo test --test integration cloud_llm_stability rule_based_and_local_evaluator_are_perfectly_stable

# 云端实验组（需真实 key，默认 #[ignore]）
DIPECS_CLOUD_LLM_API_KEY=xxx cargo test --test integration cloud_llm_stability -- --ignored --nocapture
```

**解读**：

- 确定性对照组断言：两个本地后端 10 次调用 intent 变化率 == 0%、JSON 失败率 == 0%。
- 云端实验组：统计成功返回 `IntentBatch` 的次数、JSON 解析失败次数、相邻调用间 intent 集合变化率；
  断言 JSON 失败率 ≤ 10%，且成功次数 > 1 时 intent 变化率 > 0（体现云端非确定性）。
- 用确定性本地后端替代"控制组=单次调用"的平凡基线，量化云端输出方差，
  支撑"云端仅作兜底、本地优先"的决策。

## 13. 动作成功率

**对照组**：

- 直接转发（前 DiPECS）：`direct_forward_without_policy_or_signature_succeeds` 模拟"没有
  DiPECS 治理时直接把动作转发给设备也能成功"。因 `AuthorizedAction::seal` 是
  `pub(crate)`（仅 `ActionLifecycle` 在 `PolicyEngine` 通过后可构造），无法从外部伪造，
  故走一条最小放行的 lifecycle（宽松 capability = 不被策略否决；mock bridge 不校验 HMAC =
  无签名校验），语义上等价"前 DiPECS 的直接转发"。
- mock bridge 本地接住 payload，模拟设备返回 `ok` / `rejected`。

**实验组**：`ActionLifecycle` + `AndroidBridgeAdapter` 驱动四类动作，观察 terminal state。

**运行**：

```bash
cargo test --test integration action_success_rate
```

**解读**：

- 直接转发对照组：四类可转发动作在无策略/无签名下都应 `Succeeded`——证明 DiPECS 治理
  对允许的动作不损失成功率，只是额外加了策略与签名两道门。
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
- [RFC-0002 Action Bus 治理](../rfc/0002-action-bus-governance.md)
