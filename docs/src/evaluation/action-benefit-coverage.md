# 动作收益覆盖核对与实验缺口

> Status: Assessment
> Last updated: 2026-07-04
> Purpose: 如实记录当前实验能证明什么、不能证明什么，防止把动作面覆盖度与
> 收益证明覆盖度混为一谈，也防止合成层的伪收益被当成真实系统收益引用。

## 结论先行

动作**类型是齐的**，代码链（`aios-spec` 定义 → `PolicyEngine` 能力校验 →
`AndroidAdapter` 真机派发）5 种动作全部完整。问题不在动作面，而在**收益证明的
覆盖度**：4 个真机动作只测了 2 个，且只有 `PreWarmProcess` 收益显著。

因此当前实验已经能支撑 `PreWarmProcess own:*` 的标准 LSApp split 净收益结论：
在强预测也能驱动动作的前提下，DiPECS ensemble 的 `net_benefit_ms` 为正且高于
`StrongPredictiveActionBaseline`。这个结论仍只覆盖 Android-safe 自有资源预热，
不能外推为普通 Android app 可静默预热第三方应用；其他动作的真实收益还需单独补齐。

## 真实场景证据分级

本节按「已证实 / 部分证实 / 待证实」对 DiPECS 在真实场景下的价值做分级，避免
把代码链完整度、动作可发度与最终用户收益混为一谈。数据来源为
`data/evaluation/value-metrics-20260701.md`、`data/evaluation/ux-metrics-emulator-*.md`、
`data/evaluation/ux-metrics/ux-metrics-real-device-20260704-172048.*`、
`data/evaluation/next-app/prewarm-net-benefit-real-device-20260704-184148.*`、
`data/evaluation/action-latency/action-latency-real-device-20260704-172936.*`、
`data/evaluation/resource-overhead/resource-overhead-real-device-20260704-172617.*`
以及主分支上的 CI 离线回归。

### 已证实（有真实测量支撑）

1. **隐私治理有效。** naive cloud prompt 泄漏 22 条原始通知文本、prompt 63 KB；
   DiPECS 后 0 泄漏、prompt 645 B。这是不依赖动作收益、独立成立的硬价值。
2. **本地优先路由的延迟优势真实。** RuleBased/LocalEvaluator 亚毫秒级决策，
   真实 DeepSeek API 往返 6–14 s，相差 4–5 个数量级。
3. **PreWarm 在 Android 模拟器和 Pixel 6a 上确实加速启动。** 最新 committed run
   `ux-metrics-emulator-20260703-171457` 使用 cold/prewarm 启动样本合计 n=20，
   `am start -W TotalTime` 均值从 884.1 ms 降到 489.3 ms，p95 从 932.0 ms
   降到 512.0 ms，快 44.7%。2026-07-04 Pixel 6a 真机 smoke run 复现同方向结果：
   cold startup n=5 均值 600.4 ms、p95 620.0 ms；prewarm startup n=5 均值
   142.6 ms、p95 168.0 ms，快 457.8 ms / 76.2%。
4. **PreWarm 标准 LSApp split 的净收益为正并超过强基线。** Pixel 6a n=20/mode
   net-benefit run `prewarm-net-benefit-real-device-20260704-184148` 中，collector
   cold mean/p95 为 710.75/733 ms，PreWarm hit 后为 201.55/213 ms；wrong-prewarm
   后启动 Settings 的 miss delta 为 0.5 ms，PreWarm dispatch/control cost 为
   8.394 ms/action。接入 LSApp standard hit@1 后，DiPECS ensemble
   `net_benefit_ms=75,975,810.192`，强基线为 `72,283,770.198`，DiPECS 高
   `3,692,039.994 ms`。该结论只覆盖 Android-safe `own:*` 预热证据。
5. **动作链路真实闭环。** 4 类可转发动作在 Android 模拟器/真机上均被设备确认并回执
   (`EXECUTED`)，不只是代码里能调用。Pixel 6a 真机 action bridge 回执延迟为
   841-1964 us：`PreWarmProcess own:warmup` 841 us、`KeepAlive` 973 us、
   `ReleaseMemory` 971 us、`PrefetchFile` 1964 us。
6. **系统开销够低、可常驻。** replay 1600+ 事件峰值 RSS 约 11 MB、wall time 128 ms；
   长跑 4 分钟未现显著内存增长。Pixel 6a 短窗口 resource-overhead smoke run 中，
   observe-only PSS 均值 38.946 MB，action-loop PSS 均值 40.726 MB，`top`
   CPU 读数低于该采样方法精度；这只能证明控制面开销量级较低，不能当作精确 CPU 结论。

### 部分证实（有正面数据但不足以下结论）

1. **ReleaseMemory 降 jank。** run1 降 3.67 pp，run2 完全无变化，最新 idle
   fixture 记为 `release_memory_effective=false`。Pixel 6a 真机短窗口中
   `post_release_jank` 仍为 4.76%，jank 改善 0.0 pp，PSS 降 20.418 MB。
   由于测试不是真内存压力场景，只能算弱证据，需真压力复测。
2. **云端复杂语义决策。** live DeepSeek 4 个场景全部成功产出 intent，但样本仅 4 个，
   不能说明泛化性。

### 待证实（目前只有代码路径或单次测量，不能支撑"真实场景有用"）

1. **PrefetchFile 的真实收益。** 只证明"能发出去并被确认"，没证明
   "发出去后系统变好了"。
2. **KeepAlive 的抗杀收益 —— 已确认在普通 app 形态下机制不生效（#98）。**
   KeepAlive 的系统级效果是把自身 `oom_score_adj` 调低 + pin 到 foreground
   cgroup（`SystemActionExecutors.kt`），让 LMKD 在内存压力下优先杀别人。四层
   实测证明该机制在普通 app 上无法兑现：
   - 真机普通 app：`oom=denied, cgroup=denied`，退化为 JobScheduler fallback；
   - 模拟器 root shell（uid=0）：能写 `/proc/<pid>/oom_score_adj`（机制本身存在）；
   - 模拟器 app uid（`run-as`）：写自身 `oom_score_adj` 仍 `DENIED`；
   - 模拟器 root 代写 -800 后，app 退后台 4 s 即被 AMS 重算覆盖为 50 —— app 进程的
     `oom_score_adj` 由 ActivityManagerService 动态管理，外部代写留不住。

   结论：KeepAlive 的抗杀收益只有在 platform-signed 的 `/system/bin/dipecsd`
   （不受 AMS 生命周期管理的原生进程）部署下才能兑现；app 形态下无论权限如何都
   无法验证。这与 PreWarm 的 `own:*` 边界同构。收集脚本
   `tools/collect/collect-keepalive-memory-pressure.sh` 与 n>=20 gate 已就绪，
   accept 门槛硬性要求机制真实 engage（`mechanism_engaged`），因此在系统部署可用前
   不会产出假阳性；见后续 dipecsd 系统部署 issue。
3. **ReleaseMemory 真内存压力收益。** idle 场景下结论已降级，仍缺 n>=20 真内存
   压力场景复测。
4. **真实长期用户体验。** 无真实用户、无 field study，无法支撑"用了 DiPECS 后
   电池/流畅度/启动延迟整体改善"。
5. **第三方应用静默预热收益。** 普通 Android 安全语义下 `pkg:*`/`notif:*` 不能被
   当作静默后台启动第三方 app；#90 当前关闭的是 `own:*` 自有资源预热闭环。
6. **离线 emulator fixture 的外推边界。** `action-net-benefit` fixture 能作为
   schema / CLI / CI 辅助 gate，但其中 wrong-target prewarm 成本来自既有动作确认延迟
   的保守近似，不是新的同设备多样本错预热实验；#90 的主关闭依据应以 Pixel 6a
   n=20/mode measured-device artifact 为准。

### 对外表述建议

当前最诚实的说法是：

> DiPECS 是一个能保护隐私、治理风险、低开销地把本地信号转成真实 Android 动作的
> 框架；其中 Android-safe PreWarm 已在标准 LSApp split 上证明正净收益并超过强预测基线，
> 其余动作的真实收益尚需端到端验证。

不能支撑的说法是：

> DiPECS 在真实场景下显著改善用户体验。

## 动作面核对表

`ActionType` 定义见 `crates/aios-spec/src/intent.rs`。派发见
`crates/aios-action/src/android_adapter.rs`，能力校验见
`crates/aios-core/src/policy_engine.rs`。

| ActionType | 语义 | 代码链 | 真机派发 | 收益实验 | 结论 |
| --- | --- | --- | --- | --- | --- |
| `PreWarmProcess` | 预热应用进程 | 齐 | 转发到设备 | 已测：模拟器 n=20 +44.7% 启动（489.3 vs 884.1 ms，p95 512.0 vs 932.0 ms）；Pixel 6a net-benefit n=20/mode，hit saved 509.2 ms，miss action cost 0.5 ms，dispatch/control 8.394 ms/action；DiPECS net benefit 75,975,810 ms > strong baseline 72,283,770 ms | #90 标准 split / `own:*` PreWarm gate 已闭环 |
| `ReleaseMemory` | 释放非关键内存 | 齐 | 转发到设备 | 已测但不稳定：旧 run jank -3.67 pp，新 run idle 场景 0.0 pp、PSS -0.462 MB；Pixel 6a idle jank 0.0 pp、PSS -20.418 MB，最新结论为 neutral | 收益微弱，踩「伪需求」线，暂不作卖点 |
| `PrefetchFile` | 预加载热点文件到页缓存 | 齐 | 带 `url:`/`uri:` 时转发 | 无 | 能发≠有用，收益待证 |
| `KeepAlive` | 保活当前前台进程 | 齐 | 无条件转发 | 已实测机制边界（#98）：真机/模拟器 app 形态下 `oom=denied,cgroup=denied`；root 代写 oom_score_adj 被 AMS 覆盖。app 形态无法兑现抗杀收益 | 收益需 platform-signed dipecsd 部署；app 形态下机制不生效，见 #98 |
| `NoOp` | 不执行操作 | — | — | — | — |

数据来源：`data/evaluation/ux-metrics-emulator-20260703-171457.md`、
`data/evaluation/ux-metrics/ux-metrics-real-device-20260704-172048.md`、
`data/evaluation/next-app/prewarm-net-benefit-real-device-20260704-184148.md`、
`data/evaluation/action-latency/action-latency-real-device-20260704-172936.md`、
`data/evaluation/resource-overhead/resource-overhead-real-device-20260704-172617.md`。

## 当前实验的剩余断层

按 [强 Baseline 与动作收益评估准则](strong-baseline-action-value.md) 定义的
「真价值 vs 伪需求」核对，核心链是：

```text
高质量预测 -> 及时动作 -> 设备侧终态 -> 系统指标改善
```

`PreWarmProcess own:*` 已在标准 LSApp split 上把预测命中率、设备侧 hit/miss
测量和强基线对照串起来；以下是仍不能外推的缺口。

1. **合成 action-value 是伪收益。**
   `main` 当前不包含 `crates/aios-cli/src/benchmark_next_app/action_value.rs`，也不默认输出
   `net_benefit_ms`。历史合成分支曾用
   `net_benefit_ms = 命中数 × 硬编码 120 ms − 浪费数 × 12 ms`，这类收益值不是测量结果，
   是把预测命中率乘一个假设常量再改名。若未来重新引入 action-value，必须导入真实测量数据，
   或在报告中显式标注为「合成回测常量，非真实设备测量」。
2. **LSApp 评估本身仍停在 Top-k 准确率。**
   `lsapp-standard.report.json` 证明的是预测质量；系统收益必须由
   `prewarm-net-benefit-real-device-20260704-184148` 这类导入真实设备测量的
   action-value artifact 承接，不能把纯预测报告直接当收益。
3. **PreWarm 的结论范围是 Android-safe `own:*`。**
   `ux-metrics` 实验已经补到 cold/prewarm 启动样本合计 n=20，并报告均值 + p95；
   Pixel 6a 真机 smoke run 也复现了同方向启动收益，并补了 action bridge latency
   和短窗口控制面开销；Pixel 6a n=20 net-benefit run 又补齐 miss startup delta
   与 dispatch/control cost。它不证明普通 Android 安装能静默后台启动第三方 app。

截至 `feat/strong-predictive-baseline` 的当前实验，强预测基线已能写入
`lsapp-standard.report.json` / `lsapp-coldstart.report.json`。当前 standard split 上
DiPECS ensemble 已超过强基线：hit@1 为 56.509% vs 53.784%，hit@3 为
76.059% vs 72.563%，hit@5 为 84.588% vs 80.428%。Pixel 6a n=20 net-benefit
artifact 将 standard split 的 hit@1 接入同一套实测 PreWarm hit/miss/control 成本，
并断言 DiPECS `net_benefit_ms > 0` 且高于强基线。cold-start split 上，#109 为
ensemble 新增了不依赖单用户历史的 `markov_context` / `adaptive_predictive` 全局
组件后，ensemble cold-start hit@1 由旧版 21.196% 升至 50.446%，已反超强基线
48.050%——这体现的是 RRF 融合的冷启动鲁棒性（全局分量补偿了缺失的单用户历史），
而非单用户个性化本身的贡献。

这解决 #90 在 standard split / Android-safe `own:*` PreWarm 范围内的 gate；其他动作、
第三方静默预热和长期用户体验仍必须单独评估。

`origin/main` 还包含 `data/evaluation/action-net-benefit/prewarm-emulator-20260704-measured-v1.json`
这一离线 measured fixture。它把 LSApp standard hit@1、emulator TotalTime saved latency、
设备确认延迟和离线 replay 控制面开销接入同一公式，并通过
`aios-cli generate-prewarm-net-benefit-fixture` / `compute_measured_net_benefit`
覆盖 schema、provenance 和坏数据校验。这个 fixture 适合作为 CI/schema 辅助，
但其 `wasted_prewarm_ms=31.231 ms` 来自 PreWarmProcess 设备确认延迟的保守近似，
不是 Pixel 6a wrong-target startup delta 的多样本实测，因此不替代上面的 real-device
#90 gate。

## PR #108 的 issue 归属

PR #108 (`real-device-action-evidence`) 应被理解为**证据收敛 PR**，其中
`PreWarmProcess own:*` 的标准 LSApp split gate 已闭环，ReleaseMemory 的真内存
压力复测仍未闭环:

- 对 #90:补充 Pixel 6a n=20/mode PreWarm hit/miss startup 测量、dispatch/control
  cost，并把 LSApp standard hit@1 接入 net-benefit gate。`next_app_net_benefit_test`
  会重新计算并断言 DiPECS `net_benefit_ms > 0` 且高于
  `StrongPredictiveActionBaseline`，因此关闭 #90。边界是 Android-safe `own:*`
  自有资源预热，不声称普通 Android app 可静默预热第三方应用。
- 对 #94:补充 ReleaseMemory 在真机 idle 短窗口下 jank 仍为 0.0 pp 改善的证据,
  并把 value-metrics / coverage 文档统一为「中性/弱证据/待真内存压力复测」,
  从而关闭“把 ReleaseMemory 当作稳定正收益”的数据质量问题。更严格的真内存
  压力 n>=20 复测仍由 #99 跟踪。

因此该分支关闭 #90 和 #94；ReleaseMemory 压力场景复测仍应落在 #99 的后续专门实验分支。

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
| `KeepAlive` | 避免前台进程被杀重启 | 内存压力下进程存活率 + 重启次数 | 保活挤占内存加剧他进程 OOM | 存活率提升且不恶化整体；**前提：机制须真实 engage（oom+cgroup），app 形态下恒为 fallback，需 platform-signed dipecsd** |
| `ReleaseMemory` | 缓解内存压力降 jank | **真内存压力场景**下 PSS / available / jank | 误释放导致后续重加载 | 真压力下可复现，否则降级为「中性」 |

关键修正点：

- `ReleaseMemory` 必须换场景。idle 模拟器测不出内存压力收益，需人为制造内存压力
  后再测，否则如实标注为「中性 / 待验证」。
- `PrefetchFile` 从零补起，目前只有派发路径、无任何收益证据。
- `KeepAlive` 的采集脚本 + n>=20 gate 已就绪（`collect-keepalive-memory-pressure.sh`），
  且已实测确认其抗杀机制在 app 形态下不生效（`oom_score_adj` 被 AMS 覆盖），
  正收益的验证依赖 platform-signed dipecsd 系统部署（见 #98 与后续系统部署 issue）。
- 强基线对照是硬要求，每个动作都要有「强预测直接执行、无 policy/lifecycle 治理」
  这一列，才能回答 DiPECS 的治理闭环带来什么。
- 样本量：n=5 不够，每动作每模式至少 n≥20，报均值 + p95。

优先级建议：先做 `PreWarmProcess` 完整端到端版（把已有的 +54.8% 从「预热就快」
升级为「真实命中率下净收益 vs 强基线」），再做 `PrefetchFile`。
