# 强 Baseline 与动作收益评估准则

> Status: Active
> Last updated: 2026-07-04
> Purpose: 防止评估退回 toy baseline，明确 DiPECS 要证明的系统价值。

## 结论先行

DiPECS 的主贡献不应表述为“比简单规则更会预测下一应用”，而应表述为：

> 在强预测基线已经存在的前提下，DiPECS 将上下文预测转化为受控、低开销、
> 可执行的 OS 动作闭环，并在启动延迟、动作延迟、内存压力、jank 和稳定性上产生
> 可测收益。

真正有价值的链路是：

```text
高质量预测 -> 及时动作 -> 设备侧终态 -> 系统指标改善
```

少一环都不能证明本项目的系统价值。

## 主 Baseline

主对照组应是 `StrongPredictiveActionBaseline`，代表“已有强预测技术也能直接驱动
动作”的水平，而不是原生无动作系统。

当前实现位于 `crates/aios-cli/src/next_app/strong_baseline/`，包含：

- first-order Markov：`P(next | current)`；
- second-order Markov：`P(next | previous, current)`；
- per-user frequency / MFU；
- recency：同一用户、同一 current app 下最近观察到的 next app；
- context Naive Bayes：current、previous、短 history、hour、weekday、event；
- global popularity fallback。

`StrongPredictiveActionBaseline` 必须满足：

- 使用 Android 可获得的强信号：最近前台应用、短历史、时间段、事件类型、应用使用频率等；
- 至少包含 recency/frequency、per-current-app majority、Markov；
- 输出 Top-k 预测，至少报告 Top-1 / Top-3 / Top-5；
- 在与 DiPECS 相同动作预算、相同设备、相同 trace、相同测量脚本下执行动作；
- 记录执行后的真实系统收益，而不是只记录预测准确率。

这个 baseline 回答的问题是：

> 如果已有强预测技术也能驱动动作，DiPECS 的系统控制面是否仍然带来更好的收益、
> 稳定性或错误动作控制？

## 实验矩阵

主结果表至少包含三列：

| 系统 | 预测来源 | 动作执行 | 用途 |
| --- | --- | --- | --- |
| `StrongPredictiveActionBaseline` | 强启发式 Top-k | 直接按动作预算执行 | 当前已有技术强水平 |
| `DiPECS Full Loop` | DiPECS context / router / backend | PolicyEngine + ActionLifecycle + Android bridge | 本项目系统创新 |
| `Oracle Upper Bound` | ground truth future app/action | 最优动作时机 | 上限参考，不参与胜负叙事 |

`Native no-action` 可以保留，但只能作为 sanity lower bound，不能作为主 baseline。

## 主指标

预测指标仍然重要，但必须和动作收益一起报告：

| 指标 | 说明 |
| --- | --- |
| `top1_acc` / `top3_acc` / `top5_acc` | 预测质量；Top-5 高不等于系统收益高。 |
| `prewarm_hit_rate` | 被预热应用实际被打开的比例。 |
| `wasted_prewarm_rate` | 预热后未命中的比例，代表资源浪费。 |
| `startup_total_time_delta_ms` | 启动时间改善，必须隔离 cold/warm 干扰。 |
| `startup_total_time_p95_ms` | 启动耗时 p95，避免均值掩盖尾部退化。 |
| `action_latency_ms` | intent 生成到设备侧确认动作的延迟。 |
| `memory_pressure_delta` | ReleaseMemory 前后 PSS/RSS/available memory 的变化。 |
| `jank_delta` | 动作前后 frame jank 变化。 |
| `control_plane_overhead` | DiPECS 常驻 CPU / RSS / PSS 开销。 |
| `action_success_rate` | 授权动作到设备终态的成功率。 |
| `net_benefit` | saved latency / memory benefit 减去 wasted action cost 和控制面开销。 |

主结论必须以联合指标成立：

```text
预测命中率足够高，并且动作及时执行，并且系统指标改善超过动作和控制面成本。
```

## 真价值与伪需求

### 真价值

- 强预测下，动作收益仍然可测。
- 同等 Top-k 水平下，DiPECS 的动作闭环更稳定、延迟更低或错误动作代价更小。
- 同等动作预算下，DiPECS 带来更低启动时间、更少 jank、更可控内存压力或更高终态成功率。
- 错误预测时，context window、PolicyEngine、ActionLifecycle 能减少越权动作和无效动作代价。
- Android 设备上能形成真实闭环：intent -> AuthorizedAction -> bridge -> handler -> terminal audit。

### 伪需求

- 只证明能发动作，但不证明性能收益。
- 只证明 Top-k 准，但不执行动作。
- 只和 native idle / no-action 比，并把结果当成主创新。
- PreWarm 实验混入 cold/warm process 差异，却声称因果收益。
- ReleaseMemory 没有稳定降低内存或 jank，却声称优化有效。
- 把隐私和审计作为主优化贡献。它们是系统控制面的可信部署支撑，不是主价值指标。

## 消融准则

消融必须围绕真价值，而不是为了列满表格。

| 消融 | 需要回答的问题 |
| --- | --- |
| `Full DiPECS` | 完整系统收益与成本。 |
| `No Context Window` | 单事件决策是否提高误动作或降低命中率。 |
| `No Dynamic Routing` | 固定路由是否在收益、延迟、错误动作上劣化。 |
| `No Policy Gate` | 无治理动作是否更容易越权、浪费或进入错误执行路径。 |
| `No PreWarm` | 启动收益是否消失。 |
| `No ReleaseMemory` | 内存压力或 jank 指标是否变差；若不变，则 ReleaseMemory 不应作为正面贡献。 |
| `No Android Bridge` | 只有离线 replay 是否无法证明设备端收益。 |

“纯预测不执行动作”不作为主消融。它最多作为预测上限或诊断图表，用于解释动作收益不足
是否来自预测质量。

## 当前状态

`StrongPredictiveActionBaseline` 已集成进 LSApp evaluation report，并在 CI 中有 smoke
guard 防止 report 接线回退。当前 standard split 显示 DiPECS ensemble 已超过强基线：
hit@1 为 56.509% vs 53.784%，hit@3 为 76.059% vs 72.563%，hit@5 为
84.588% vs 80.428%。

PR #108 又补充了 Pixel 6a n=20/mode PreWarm hit/miss measurement：
`prewarm-net-benefit-real-device-20260704-184148` 用 `am start -W TotalTime`
测得 hit saved latency 509.2 ms、wrong-prewarm miss startup delta 0.5 ms、
PreWarm dispatch/control cost 8.394 ms/action。`next_app_net_benefit_test`
现在用这些 measured-device inputs 重新计算 net benefit，并断言 DiPECS ensemble
`net_benefit_ms` 为正且高于 `StrongPredictiveActionBaseline`：
75,975,810.192 ms vs 72,283,770.198 ms。

`origin/main` 还包含一个离线 action-level measured fixture gate，可作为 schema / CLI /
CI 辅助，而不是替代上面的 real-device gate：

- fixture：`data/evaluation/action-net-benefit/prewarm-emulator-20260704-measured-v1.json`
- 生成入口：`aios-cli generate-prewarm-net-benefit-fixture`
- 计算入口：`compute_measured_net_benefit`
- CI 覆盖：`next_app_net_benefit_test` 中的 fixture validation、CLI generation、
  DiPECS-vs-strong measured net-benefit gate，以及大量坏数据/边界矩阵测试。

当前 measured fixture 使用：

| 字段 | 数值 | 解释 |
| --- | ---: | --- |
| `prewarm_saved.mean_ms` | 394.8 | emulator `am start -W TotalTime` cold/prewarm 均值差 |
| `wasted_prewarm.mean_ms` | 31.231 | PreWarmProcess 设备确认延迟，作为错预热成本的保守近似 |
| `control_plane.dipecs.mean_ms` | 0.07848 | replay 128.0 ms / 1631 events 摊销 |
| `control_plane.strong_baseline.mean_ms` | 0.0 | 对强基线有利的下界 |

这比 gross-saved gate 前进一步：`net_benefit` 不再依赖无来源 placeholder，且测试会拒绝
placeholder source、0 samples、负值、空 provenance、坏 report/UX fixture 等常见坏数据。

但这个 fixture 仍需按边界解读：它是**离线 measured fixture gate**，不是新采集的
同设备多样本 wrong-target prewarm 实验。尤其 `wasted_prewarm` 目前采用既有设备确认延迟，
尚未把错预热后的 CPU/RSS/PSS 资源浪费折算进 ms。

Pixel 6a measured-device gate 关闭 #90 在 standard LSApp split 与 Android-safe `PreWarmProcess own:*`
范围内的要求。它不声称普通 Android app 可以静默预热第三方应用，也不替代其他动作的
收益评估。

后续动作仍必须按同一公式补齐真实 action-level net benefit：

```text
net_benefit = measured_saved_latency - measured_wasted_action_cost - measured_control_plane_cost
```

每个 gate 只有在同设备、同 trace、同动作预算下，DiPECS 的净收益为正且优于
`StrongPredictiveActionBaseline` 时才应默认启用。
离线 fixture gate 可以保留为 CI/schema 回归；论文/对外叙述若要声称完整真实设备净收益，
必须引用同设备、同 trace、同动作预算下的 measured-device gate，或补齐新的多样本
wrong-target prewarm 成本和资源开销。
