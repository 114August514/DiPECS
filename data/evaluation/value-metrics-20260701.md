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
| Audit hash | `sha256:2b3c5ac19314ac5128910fd26db3e02e76291cd495ee6fe87552a2b26ea7cde2` |

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

- **延迟**:放本地 vs 云端柱状图,数量级差异(~7s vs <0.1ms)。
- **隐私**:放"naive prompt 63KB / DiPECS 645B" + "22 leaks / 0 leaks" 对比。
- **资源**:放"1631 events / 128 ms / 10.8MB RSS" 三数字。
- **治理**:放 policy_engine_test 20/20 + denial reasons 列表。
- **UX 动作延迟**:放四类型设备确认延迟(KeepAlive ~21ms,ReleaseMemory ~13ms,PreWarm ~31ms,Prefetch ~1ms)。

## 8. 已补充

- `tests/scenarios/action-latency-sweep.sh`:已在 Android Emulator `dipecs_e2e` 上运行,
  四类动作设备侧确认延迟见第 5 节表格。
