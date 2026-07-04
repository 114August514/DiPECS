# 动作收益覆盖核对与实验缺口

> Status: Assessment
> Last updated: 2026-07-04
> Purpose: 如实记录当前实验能证明什么、不能证明什么，防止把动作面覆盖度与
> 收益证明覆盖度混为一谈，也防止合成层的伪收益被当成真实系统收益引用。

## 结论先行

动作**类型是齐的**，代码链（`aios-spec` 定义 → `PolicyEngine` 能力校验 →
`AndroidAdapter` 真机派发）5 种动作全部完整。问题不在动作面，而在**收益证明的
覆盖度**：4 个真机动作只测了 2 个，且只有 `PreWarmProcess` 收益显著。

因此当前实验能支撑的最强表述只是「预测准 + 预热确实快」，还不足以支撑本项目的
核心创新叙事——「在强预测也能驱动动作的前提下，DiPECS 的治理闭环带来可测的
净收益 / 更低的错误动作代价」。补齐路径见本文末。

## 真实场景证据分级

本节按「已证实 / 部分证实 / 待证实」对 DiPECS 在真实场景下的价值做分级，避免
把代码链完整度、动作可发度与最终用户收益混为一谈。数据来源为
`data/evaluation/value-metrics-20260701.md`、`data/evaluation/ux-metrics-emulator-*.md`
以及主分支上的 CI 离线回归。

### 已证实（有真实测量支撑）

1. **隐私治理有效。** naive cloud prompt 泄漏 22 条原始通知文本、prompt 63 KB；
   DiPECS 后 0 泄漏、prompt 645 B。这是不依赖动作收益、独立成立的硬价值。
2. **本地优先路由的延迟优势真实。** RuleBased/LocalEvaluator 亚毫秒级决策，
   真实 DeepSeek API 往返 6–14 s，相差 4–5 个数量级。
3. **PreWarm 在 Android 模拟器上确实加速启动。** 最新 committed run
   `ux-metrics-emulator-20260703-171457` 使用 cold/prewarm 启动样本合计 n=20，
   `am start -W TotalTime` 均值从 884.1 ms 降到 489.3 ms，p95 从 932.0 ms
   降到 512.0 ms，快 44.7%。
4. **动作链路真实闭环。** 4 类可转发动作在 Android 模拟器/真机上均被设备确认并回执
   (`EXECUTED`)，不只是代码里能调用。
5. **系统开销够低、可常驻。** replay 1600+ 事件峰值 RSS 约 11 MB、wall time 128 ms；
   长跑 4 分钟未现显著内存增长。

### 部分证实（有正面数据但不足以下结论）

1. **ReleaseMemory 降 jank。** run1 降 3.67 pp，run2 完全无变化，最新 idle
   fixture 记为 `release_memory_effective=false`。由于测试不是真内存压力场景，只能算弱证据，需真压力复测。
2. **云端复杂语义决策。** live DeepSeek 4 个场景全部成功产出 intent，但样本仅 4 个，
   不能说明泛化性。

### 待证实（目前只有代码路径或单次测量，不能支撑"真实场景有用"）

1. **PrefetchFile / KeepAlive 的真实收益。** 只证明"能发出去并被确认"，没证明
   "发出去后系统变好了"。
2. **DiPECS 相对强基线的离线净收益。** `StrongPredictiveActionBaseline` 已接入
   LSApp 评估，PreWarmProcess 已有 committed action-net-benefit fixture，把
   LSApp standard hit@1、emulator TotalTime saved latency、设备确认延迟和离线 replay
   控制面开销接成一个非 placeholder gate。但它仍不是新的同设备 wrong-target 多样本实验。
3. **真实长期用户体验。** 无真实用户、无 field study，无法支撑"用了 DiPECS 后
   电池/流畅度/启动延迟整体改善"。
4. **预测→动作→收益的端到端链。** LSApp 真实数据只到预测准确率；ux-metrics 只到
   "预热就快"；`action-net-benefit` fixture 已把二者与实测成本连接起来，但设备侧
   wrong-target prewarm 成本仍来自既有动作确认延迟的保守近似，不是新采集的多样本错预热实验。

### 对外表述建议

当前最诚实的说法是：

> DiPECS 是一个能保护隐私、治理风险、低开销地把本地信号转成真实 Android 动作的
> 框架；其中 PreWarm 在模拟器上显著加速启动，其余动作的真实收益尚需端到端验证。

不能支撑的说法是：

> DiPECS 在真实场景下显著改善用户体验。

## 动作面核对表

`ActionType` 定义见 `crates/aios-spec/src/intent.rs`。派发见
`crates/aios-action/src/android_adapter.rs`，能力校验见
`crates/aios-core/src/policy_engine.rs`。

| ActionType | 语义 | 代码链 | 真机派发 | 收益实验 | 结论 |
| --- | --- | --- | --- | --- | --- |
| `PreWarmProcess` | 预热应用进程 | 齐 | 转发到设备 | 已测：+44.7% 启动（489.3 vs 884.1 ms，`am start -W TotalTime`，p95 512.0 vs 932.0 ms） | 真闪光点 |
| `ReleaseMemory` | 释放非关键内存 | 齐 | 转发到设备 | 已测但不稳定：旧 run jank -3.67 pp，新 run idle 场景 0.0 pp、PSS -0.462 MB，最新结论为 neutral | 收益微弱，踩「伪需求」线，暂不作卖点 |
| `PrefetchFile` | 预加载热点文件到页缓存 | 齐 | 带 `url:`/`uri:` 时转发 | 无 | 能发≠有用，收益待证 |
| `KeepAlive` | 保活当前前台进程 | 齐 | 无条件转发 | 无 | 能发≠有用，收益待证 |
| `NoOp` | 不执行操作 | — | — | — | — |

数据来源：`data/evaluation/ux-metrics-emulator-20260703-171457.md`。

## 当前实验的三个断层

按 [强 Baseline 与动作收益评估准则](strong-baseline-action-value.md) 定义的
「真价值 vs 伪需求」核对，核心链是：

```text
高质量预测 -> 及时动作 -> 设备侧终态 -> 系统指标改善
```

三环目前各自成立，却**从未在同一条 trace、同一台设备上端到端串起来**。

1. **合成 action-value 是伪收益。**
   `main` 当前不包含 `crates/aios-cli/src/benchmark_next_app/action_value.rs`，也不默认输出
   `net_benefit_ms`。历史合成分支曾用
   `net_benefit_ms = 命中数 × 硬编码 120 ms − 浪费数 × 12 ms`，这类收益值不是测量结果，
   是把预测命中率乘一个假设常量再改名。若未来重新引入 action-value，必须导入真实测量数据，
   或在报告中显式标注为「合成回测常量，非真实设备测量」。
2. **LSApp 评估停在 Top-k 准确率，不发动作。**
   命中率很高（standard 集 ensemble hit@1 = 56.442%），但按准则「只证明 Top-k 准、
   不执行动作」属伪需求。它证明的是预测质量，不是系统收益。
3. **PreWarm 收益是「预热了就快」，接近同义反复。**
   `ux-metrics` 实验已经补到 cold/prewarm 启动样本合计 n=20，并报告均值 + p95；
   standard split 已把真实预测命中率接入 gross-saved gate；但它仍没有测
   误预热成本和控制面开销，因此不能把 placeholder net benefit 当作完整净收益。

此外，最关键的缺口是**没有与强基线在同设备同预算下对打**：准则要求主对照是
`StrongPredictiveActionBaseline`（强预测也来驱动动作），而不是 native no-action。

截至 `feat/strong-predictive-baseline` 的当前实验，强预测基线已能写入
`lsapp-standard.report.json` / `lsapp-coldstart.report.json`。当前 standard split 上
DiPECS ensemble 已超过强基线：hit@1 为 56.442% vs 53.784%，hit@3 为
76.104% vs 72.563%，hit@5 为 84.241% vs 80.428%；因此可以启用
「预测命中率 × 实测 PreWarm 加速」的 gross-saved 先决 gate。cold-start 仍不能作为
DiPECS ensemble 胜出证据：hit@1 为 21.196% vs 48.050%。

`data/evaluation/action-net-benefit/prewarm-emulator-20260704-measured-v1.json` 进一步补上了
#90 的离线 measured gate：

| 输入 | 数值 | 来源 | 说明 |
| --- | ---: | --- | --- |
| DiPECS ensemble hit@1 | 56.442% | `lsapp-standard.report.json` | standard split |
| StrongPredictive hit@1 | 53.784% | `lsapp-standard.report.json` | strong baseline 同 test window |
| PreWarm saved latency | 394.8 ms | `ux-metrics-emulator-20260703-171457.json` | `am start -W TotalTime`，cold/prewarm 合计 n=20 |
| Wasted PreWarm cost | 31.231 ms | `value-metrics-20260701.md` | PreWarmProcess 设备确认延迟，作为错预热成本的保守近似 |
| DiPECS control-plane cost | 0.07848 ms / prediction | `value-metrics-20260701.md` | replay 128.0 ms / 1631 events 摊销 |
| Strong baseline control-plane cost | 0.0 ms / prediction | `lsapp-standard.report.json` | 对 baseline 有利的下界 |

对应公式为：

```text
net_benefit =
  examples * hit@1 * prewarm_saved_ms
  - examples * (1 - hit@1) * wasted_prewarm_ms
  - examples * control_plane_ms
```

在 `test_examples = 272519` 下，当前 fixture gate 要求 DiPECS PreWarm measured net
benefit 为正，并且优于 `StrongPredictiveActionBaseline`。这个 gate 已由
`crates/aios-cli/tests/next_app_net_benefit_test.rs` 非 ignored 测试覆盖。

边界仍需写清：这不是新的真机多样本 wrong-target prewarm 实验；`wasted_prewarm_ms`
目前用既有设备确认延迟作为保守近似，RSS/PSS/CPU 资源浪费没有折算进 ms。若未来采集
wrong-target 多样本，应替换 fixture，而不是绕过 schema。

生成命令示例：

```bash
cargo run -p aios-cli -- generate-prewarm-net-benefit-fixture \
  --report data/evaluation/next-app/lsapp-standard.report.json \
  --ux-metrics data/evaluation/ux-metrics/ux-metrics-emulator-20260703-171457.json \
  --output data/evaluation/action-net-benefit/prewarm-emulator-YYYYMMDD.json \
  --dataset-id prewarm-emulator-YYYYMMDD \
  --wasted-prewarm-ms 31.231 \
  --wasted-prewarm-samples 1 \
  --dipecs-control-plane-ms 0.07848 \
  --dipecs-control-plane-samples 1631 \
  --strong-control-plane-ms 0.0 \
  --strong-control-plane-samples 272519
```

## 补齐路径：分动作 net-benefit 实验

总原则：每个动作单独定义收益机制、测量手段、浪费代价、对照组，禁止无来源硬编码常量，
禁止合并成单一笼统的 net_benefit。同一设备、同一 trace、同一动作预算。当前
`synthetic-next-app-v1.report.json` 是预测回归报告，只能验证「预测→动作候选」映射；
默认不包含 `net_benefit_ms`，也不能作为真实设备收益引用。

通用骨架：

```text
真实 trace（LSApp 或 android_real_device_sample）
  -> DiPECS 产出真实预测 + intent（真实命中率）
  -> PolicyEngine 授权（记录 blocked 数）
  -> 真机执行动作
  -> 实测设备侧终态
  -> net_benefit = 实测收益 - 实测浪费 - 控制面开销
对照列：StrongPredictiveActionBaseline（Markov/frequency 直接执行，无治理）
```

各动作的测量定义：

| 动作 | 收益机制 | 实测手段 | 浪费代价 | 及格线 |
| --- | --- | --- | --- | --- |
| `PreWarmProcess` | 命中时省冷启动 | `am start -W` TotalTime，命中/未命中分别测 | 未命中预热的 CPU/RSS 占用 | 净收益 > 0 且优于强基线 |
| `PrefetchFile` | page cache 命中降 IO 延迟 | 预取后读延迟对比（`vmtouch` / 首读 wall time） | 未命中文件占页缓存、被回收 | 命中率 + IO delta 显著 |
| `KeepAlive` | 避免前台进程被杀重启 | 内存压力下进程存活率 + 重启次数 | 保活挤占内存加剧他进程 OOM | 存活率提升且不恶化整体 |
| `ReleaseMemory` | 缓解内存压力降 jank | **真内存压力场景**下 PSS / available / jank | 误释放导致后续重加载 | 真压力下可复现，否则降级为「中性」 |

关键修正点：

- `ReleaseMemory` 必须换场景。idle 模拟器测不出内存压力收益，需人为制造内存压力
  后再测，否则如实标注为「中性 / 待验证」。
- `PrefetchFile` / `KeepAlive` 从零补起，它们目前只有派发路径、无任何收益证据。
- 强基线对照是硬要求，每个动作都要有「强预测直接执行、无 policy/lifecycle 治理」
  这一列，才能回答 DiPECS 的治理闭环带来什么。
- 样本量：n=5 不够，每动作每模式至少 n≥20，报均值 + p95。

优先级建议：先做 `PreWarmProcess` 完整端到端版（把已有的 +54.8% 从「预热就快」
升级为「真实命中率下净收益 vs 强基线」），再做 `PrefetchFile`。
