# DiPECS 价值指标报告

生成时间: 2026-07-01

本报告汇总当前代码可验证的价值数据,供演示 PPT 引用。所有数据来自真实测试,
非估算。

## 1. 决策延迟:本地后端 vs CloudLLM(真实 DeepSeek API)

测试:`crates/aios-agent/src/backends/cloud_llm/mod.rs::cloud_llm::latency_tests::decision_latency_comparison`
运行方式:`DIPECS_CLOUD_LLM_API_KEY=... DIPECS_BENCH_ROUNDS=10 cargo test -p aios-agent --lib cloud_llm::latency_tests::decision_latency_comparison -- --ignored --nocapture`
模型:`deepseek-v4-flash`

| 后端 | 均值 | p50 | p95 |
| --- | --- | --- | --- |
| RuleBased | 0.00 ms | 0.00 ms | 0.02 ms |
| LocalEvaluator | 0.01 ms | 0.01 ms | 0.05 ms |
| CloudLLM(DeepSeek deepseek-v4-flash) | 6958.16 ms | 7339.61 ms | 10050.08 ms |

**价值**:本地规则/轻量模型可在亚毫秒级完成决策,而云端 LLM 往返需要数秒。
DiPECS 默认优先走本地路由,仅在复杂语义时才考虑云端,显著降低交互延迟。

### 1.1 云端后端直采 API 实测(DeepSeek live)

数据:`data/evaluation/cloud-latency-20260716-084110.json`、`cloud-scenarios-20260716-084010.json`
(`status: measured_live_api`;数据集 ID 内的 `20260716` 系生成机时钟偏差,真实采集/提交日期为
2026-07-01)。模型 `deepseek-v4-flash`,provider DeepSeek。

延迟(morning-routine,5 轮):

| 指标 | 数值 |
| --- | --- |
| p50 | 11331 ms |
| p95 | 12963 ms |
| min / max | 5527 / 12963 ms |
| 成功率 | 100.0% |
| 错误数 | 0 |

场景决策质量(4 个真实场景,全部成功产出 intent、0 错误):

| 场景 | 产出 intent | 延迟 |
| --- | --- | --- |
| circuit-breaker | Idle | 9180 ms |
| low-battery | Idle | 6213 ms |
| morning-routine | CheckNotification + HandleFile(Image) | 14175 ms |
| rich-workflow | SwitchToApp(com.example.chat) | 10012 ms |

**价值**:真实云端后端(非 mock)能端到端产出结构化 intent,佐证第 1 节的量级对比——
云端往返 6–14 秒,与本地亚毫秒决策相差 4–5 个数量级,支撑"本地优先、云端仅兜复杂语义"的路由策略。

## 2. 资源开销:replay 2400 行大 trace

测试:`crates/aios-cli/tests/resource_overhead_test.rs::replay_large_trace_resource_overhead`
Trace:`data/traces/android_synthetic_large.redacted.jsonl`(2400 行,1631 条有效事件)

| 指标 | 数值 |
| --- | --- |
| Wall time | 128.0 ms |
| Peak RSS | 10.77 MB |
| CPU user time | 120.0 ms |
| CPU system time | 0.0 ms |
| Events ingested | 1631 |
| Windows closed | 58 |
| Actions authorized | 206 |
| Throughput | 12742.2 events/s |
| Audit hash | `sha256:20ead9900295210d6d279fe685f53bdd02f084d374fcddb82e96a05244ac4f85` |

**价值**:DiPECS 核心管线极轻量,处理 1600+ 事件峰值内存仅约 11MB,百毫秒内完成,
不会成为设备常驻服务的资源负担。

## 3. 隐私边界:有/无 DiPECS 对比

测试:`crates/aios-agent/tests/baseline_comparison_test.rs::baseline_privacy_and_governance_comparison`
场景:`data/traces/scenarios/circuit-breaker.jsonl`

| 指标 | 无 DiPECS(naive cloud prompt) | 有 DiPECS |
| --- | --- | --- |
| 原始通知文本片段数 | 300 | 300 |
| 泄漏到模型输入/审计的数量 | 22 | **0** |
| Prompt/模型输入大小 | 63178 bytes | 645 bytes |

辅助测试:

- `crates/aios-cli/tests/replay_audit_leak_test.rs`:2/2 pass,审计流与 NDJSON 输出均不含 raw notification PII。
- `crates/aios-cli/tests/golden_trace_integration_test.rs`:6/6 pass,确定性审计 hash 可捕获任何管道状态变化。

**价值**:DiPECS 的 `PrivacyAirGap` 确保 raw_title/raw_text 等敏感内容不会流入模型输入
或审计流,同时把输入从 63KB 压缩到 645B,既保护隐私又降低模型成本。

## 4. 治理边界:策略引擎拦截能力

测试:`crates/aios-core/tests/policy_engine_test.rs`(20 项全部通过)

覆盖的治理规则:

- 高风险动作默认拒绝
- 中风险动作按 capability 配置决定
- 低置信度 intent 拒绝
- target 不在 context 中拒绝
- 每 batch 最大动作数限制
- deferred urgency 过滤
- FallbackNoOp 拦截 PreWarm

**价值**:即使本地或云端后端建议了动作,`PolicyEngine` 仍按风险等级、capability、
target-in-context 等多重规则进行二阶审查,防止越权执行。

## 5. 动作覆盖与真机执行证据

测试/脚本:

- `crates/aios-action/tests/android_bridge_e2e_test.rs`:mock-socket 端到端覆盖 4 类型
- `tests/scenarios/action-loop-e2e.sh`:真机/模拟器 EXECUTED 验证
- `tests/scenarios/action-latency-sweep.sh`:设备侧动作确认延迟(需 emulator/真机)

| 动作类型 | 设备终态审计事件 | 设备确认延迟(us) | 状态 |
| --- | --- | --- | --- |
| KeepAlive | `keep_alive_scheduled` → `keep_alive_job_executed` | 21343 | EXECUTED |
| ReleaseMemory | `release_memory_completed` | 13409 | EXECUTED |
| PreWarmProcess | `own_resources_prewarmed` | 31231 | EXECUTED |
| PrefetchFile | `prefetch_started` → `prefetch_succeeded` | 1069 | EXECUTED |

**环境**:Android Emulator `dipecs_e2e`,host x86_64,`adb forward tcp:46321 tcp:46321`。
**注**:PrefetchFile 为异步派发,回执仅代表已入队,因此延迟显著低于同步动作。

**价值**:不是"代码里能调用",而是四类可转发动作在真实 Android 环境中均被设备确认
并执行,链路(`AndroidAdapter` → execute 信封 → HMAC 校验 → dispatch → handler)完整闭环。

## 6. 信号→动作映射覆盖

测试:`crates/aios-cli/tests/noop_matrix_test.rs`

| 统计 | 数值 |
| --- | --- |
| 信号模式总数 | 13 |
| 产生真实动作的模式 | 9 (69.2%) |
| 当前为 NoOp 的盲区 | 4 (30.8%) |

产生真实动作的典型模式:

- `ok:app_foreground_keepalive` → PreWarmProcess + KeepAlive
- `local:file_access_prefetch` → PrefetchFile
- `ok:low_battery_release_memory` → ReleaseMemory
- `ok:screen_interactive_keepalive` → KeepAlive

**价值**:系统能把常见设备语义(前台应用、文件访问、低电量、屏幕交互等)映射为
对应的维护动作,而非简单 Idle。

## 7. 使用建议(PPT 引用)

- **延迟**:放本地 vs 云端柱状图,数量级差异(~7s vs <0.1ms);云端可用 live DeepSeek p50 11.3s(§1.1)。
- **隐私**:放"naive prompt 63KB / DiPECS 645B" + "22 leaks / 0 leaks" 对比。
- **资源**:放"1631 events / 128 ms / 10.8MB RSS" 三数字;设备内可补"每叠一层 +7–8MB RSS、jank 0"(§8.1)。
- **治理**:放 policy_engine_test 20/20 + denial reasons 列表。
- **UX 动作延迟**:放四类型设备确认延迟(KeepAlive ~21ms,ReleaseMemory ~13ms,PreWarm ~31ms,Prefetch ~1ms)。
- **启动加速**:放 warm 1470ms → prewarm 665ms(快 54.8%),PreWarm 的 UX 收益(§8.3)。
- **稳定性**:放 4 分钟长跑 RSS 无增长、PSS 3.9MB/h < 阈值,支撑"可常驻"(§8.2)。

## 8. 补充实测数据(设备内 / 长跑 / UX)

> 以下为 value-metrics 首版之后补测的设备内数据(均在 Android 模拟器 `dipecs_e2e`,
> android-35 x86_64 上采集)。2026-07-04 又补充了 Pixel 6a 真机 smoke run 和
> n=20/mode PreWarm net-benefit run，用于关闭 #90 在 standard LSApp split 与
> Android-safe `own:*` PreWarm 范围内的 gate。

- `tests/scenarios/action-latency-sweep.sh`:已在模拟器运行,四类动作设备侧确认延迟见第 5 节表格。

### 8.1 设备内资源开销(模拟器,30 样本/模式)

数据:`data/evaluation/resource-overhead-emulator-20260701-162742.json`
(30 样本/模式,较早先 10 样本的 `-131525` 更稳)。

| 模式 | Avg CPU | Avg RSS | Avg PSS | Avg jank |
| --- | ---: | ---: | ---: | ---: |
| baseline_idle | 0.493% | 118.30 MB | 36.02 MB | 0.0% |
| dipecs_observe_only | 0.387% | 125.87 MB | 39.63 MB | 0.0% |
| dipecs_action_loop | 0.0% | 132.80 MB | 41.62 MB | 0.0% |

**读法(诚实)**:模拟器上 CPU 占用落在测量噪声内(两次运行分别测得约 1.15% 与约 0.4%/0,
甚至出现"观测 < 基线"的负差),`dipecs_action_loop = 0.0%` 不能单独引用为精确 CPU 结论;
**稳定可报的是 RSS 每叠一层约 +7–8 MB**(采集 +7.6 MB,动作回路再 +6.9 MB),
jank 全 0。电量/温度为 AC 供电下的换算估算,非燃料计实测。

### 8.2 运行稳定性(长跑无内存泄漏)

数据:`data/evaluation/stability-emulator-canonical.json`(4 分钟、8 样本、30s 间隔)。

| 指标 | 数值 | 阈值 | 结论 |
| --- | ---: | ---: | --- |
| RSS 变化 | −5.41 MB | — | 未增长 |
| PSS 增长/小时 | 3.91 MB/h | < 20 | 通过 |
| RSS 增长/小时 | 6.08 MB/h | < 50 | 通过 |
| 平均 CPU | 0.95% | < 10 | 通过 |

**价值**:短时长跑未见显著内存增长(RSS 甚至回落),支撑"可作设备常驻服务"的论点。
(注:4 分钟为短窗观测,长期泄漏需更长跑验证。)

### 8.3 UX 收益:PreWarm 启动加速 / ReleaseMemory 语义升级

模拟器数据:`ux-metrics-emulator-20260701-150110.json`(run1)、
`-151856.json`(run2),各 5 样本/模式。

| 指标 | run1 | run2 |
| --- | ---: | ---: |
| warm 启动 TotalTime | 1470.4 ms | 1551.6 ms |
| prewarm 启动 TotalTime | 664.6 ms | 872.6 ms |
| **PreWarm 加速** | **805.8 ms(54.8%)** | **679.0 ms(43.8%)** |
| ReleaseMemory 前 jank | 19.05% | 30.0% |
| ReleaseMemory 后 jank | 15.38% | 30.0% |
| ReleaseMemory jank 改善 | 3.67 pp | 0.0 pp |

**读法(诚实)**:**PreWarm 两轮一致显著**(启动快 44–55%),是最硬的 UX 收益证据;
**ReleaseMemory 降 jank 两轮不一致**(run1 有、run2 无),最新 idle run 记为
`release_memory_effective=false` / neutral。旧 `cache:prefetch` 真压力复测
`release-memory-pressure-benefit-20260705-173505` accepted=false，证明删磁盘缓存文件
不能作为内存压力收益；本次语义升级后的 `cache:volatile` 真压力复测
`release-memory-pressure-benefit-20260705-185226` accepted=true，可作为 app-owned
volatile memory release 的正面证据。

后续 committed emulator run `ux-metrics-emulator-20260703-171457` 把 cold/prewarm
启动样本合计补到 n=20:冷启动均值 884.1 ms、p95 932.0 ms;prewarm 均值
489.3 ms、p95 512.0 ms,快 394.8 ms / 44.7%。同一 idle 场景下 ReleaseMemory
jank 改善仍为 0.0 pp、PSS -0.462 MB,结论保持 neutral。

Pixel 6a 真机 smoke run `data/evaluation/ux-metrics/ux-metrics-real-device-20260704-172048.md`
复现了 PreWarm 同方向收益:cold startup n=5 均值 600.4 ms、p95 620.0 ms;
prewarm startup n=5 均值 142.6 ms、p95 168.0 ms,快 457.8 ms / 76.2%。
但 ReleaseMemory 在 idle 短窗口中 jank 仍为 4.76%,改善 0.0 pp,仅观察到 PSS
降低 20.418 MB。2026-07-05 Pixel 6a n=20/mode 真压力复测分成两层结论：
旧 `cache:prefetch` available-memory gain -3475.4 KB、PSS reduction gain -2205.2 KB、
Welch p=0.65937954，accepted=false；升级后的 `cache:volatile` 在
`PreWarmProcess own:volatile-cache:64` seed 后释放 app-owned volatile cache，
available-memory gain +55158.6 KB、PSS reduction gain +64621.3 KB、jank delta
0.0 pp、Welch p=0.00026891，accepted=true。因此 ReleaseMemory 当前可作为
**app-owned volatile memory release** 的正面证据引用，但不得表述为稳定 jank/长期 UX 卖点。

Pixel 6a n=20/mode net-benefit run
`data/evaluation/next-app/prewarm-net-benefit-real-device-20260704-184148.md`
进一步把 #90 的 PreWarm gate 串起来：collector cold mean/p95 为 710.75/733 ms，
PreWarm hit 后为 201.55/213 ms，hit saved latency 509.2 ms；wrong-prewarm
后启动 Settings 的 miss startup delta 为 0.5 ms，PreWarm dispatch/control cost 为
8.394 ms/action。接入 LSApp standard hit@1 后，DiPECS ensemble
`net_benefit_ms=75,975,810.192`，`StrongPredictiveActionBaseline`
为 `72,283,770.198`，DiPECS 高 `3,692,039.994 ms`。该结论只覆盖 Android-safe
`own:*` 自有资源预热，不声称普通 Android app 可静默预热第三方应用。

### 8.4 云端直采 API

见 §1.1(延迟 + 场景决策质量,live DeepSeek)。
