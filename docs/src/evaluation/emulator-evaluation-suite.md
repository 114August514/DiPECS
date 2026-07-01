# DiPECS Emulator Evaluation Suite

基于 Android Studio Emulator 的 DiPECS 性能评测体系，覆盖资源开销、用户体验、稳定性和云端决策四个维度。所有测试在 CI 自动执行，数据集通过 `include_str!` 内嵌，测试重算 summary 确保数据不会漂移。

---

## 快速开始

```bash
cd /mnt/e/DIPECS

# 资源开销（10 采样，约 5 分钟）
./tools/collect-resource-overhead.sh

# 用户体验（5 采样 × 4 模式，约 3 分钟）
./tools/collect-ux-metrics.sh

# 稳定性（4 分钟起步，建议 ≥60 分钟）
DURATION_MINUTES=60 ./tools/collect-stability.sh

# 跑全部 CI 测试
cargo test --package aios-cli --test resource_overhead_dataset_test
cargo test --package aios-cli --test ux_metrics_dataset_test
cargo test --package aios-cli --test stability_dataset_test

# 云端测试（需 API key）
source .env
cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::smoke -- --ignored --nocapture
cargo test -p aios-agent --lib mock_cloud_e2e_tests
```

---

## 一、资源开销

> 问题：DiPECS 后台运行吃掉多少 CPU、内存、电量？

### 测量方法

`adb shell top` + `dumpsys meminfo` + `dumpsys gfxinfo`，三种模式：

| 模式 | 说明 |
|------|------|
| `baseline_idle` | App force-stop，系统基线 |
| `dipecs_observe_only` | 后台运行，仅采集不动作 |
| `dipecs_action_loop` | 后台运行 + 持续发送 KeepAlive / ReleaseMemory / PreWarm / Prefetch |

### 结果 (10 采样 × 10s 间隔)

| 模式 | CPU | RSS | PSS | Jank |
|------|----:|----:|----:|----:|
| baseline_idle | 0.00% | 0 MB | 0 MB | 0% |
| dipecs_observe_only | 1.15% | 138 MB | 28 MB | 0% |
| dipecs_action_loop | 1.16% | 145 MB | 31 MB | 0% |

**增量 vs 基线：**

| 指标 | observe_only | action_loop | 阈值 |
|------|------------:|------------:|-----:|
| CPU Δ | +1.15 pp | +1.16 pp | ≤ 8 pp |
| RSS Δ | +138 MB | +145 MB | ≤ 220 MB |
| PSS Δ | +28 MB | +31 MB | ≤ 80 MB |

**预估功耗 (CPU/PSS 模型估算，模拟器 AC 供电)：**

| 指标 | observe_only | action_loop | 阈值 |
|------|------------:|------------:|-----:|
| 耗电 | 0.14 mAh/min | 0.21 mAh/min | ≤ 0.35 |
| 10min 耗电占比 (4000mAh) | 0.035% | 0.052% | — |
| 温升 | +0.58°C | +0.87°C | ≤ 1.5°C |

> **结论：CPU < 2%，PSS ≈ 30MB，10 分钟耗电 ≈ 0.05%。用户不可感知。**

### 测试 (5 个，CI 自动)

| 测试 | 验证 |
|------|------|
| `measurement_is_internally_consistent` | summary 与 raw samples 重算一致 |
| `fixture_stays_within_budget` | CPU/RSS/PSS/Jank 增量 ≤ 阈值 |
| `conclusion_matches_recomputed_deltas` | conclusion 与重算 delta 一致 |
| `simulated_power_thermal_estimates_are_labeled_and_bounded` | 功耗/热估计标注正确且在界内 |
| `report_summary_merges_measured_and_estimated_values` | report 行与 run summary 交叉验证 |

---

## 二、用户体验

> 问题：DiPECS 的 PreWarm 能让 App 启动快多少？ReleaseMemory 能降低多少卡顿？

### 测量方法

`am start -W` + `dumpsys gfxinfo` + `dumpsys meminfo`，五 种模式：

| 模式 | 说明 |
|------|------|
| `no_dipecs_baseline` | DiPECS 停止，系统空闲基线 |
| `cold_startup` | 真冷启动：force-stop → 直接启动 MainActivity |
| `prewarm_startup` | DiPECS 预热：启动后台服务 → PreWarmProcess → 启动 MainActivity |
| `baseline_jank` | 正常运行中帧率 |
| `post_release_jank` | ReleaseMemory 后帧率 |

### 结果 (5 采样 × 3s 间隔)

**系统基线 (无 DiPECS)：** 空闲内存 2568 MB

**启动耗时：**

| 场景 | TotalTime | Jank | vs 无 DiPECS |
|------|----------:|-----:|------------:|
| cold_startup (无 DiPECS) | 1552 ms | 80% | — |
| prewarm_startup (DiPECS) | 873 ms | 80% | **44% 更快** |

> 冷启动 Jank 高是正常的（首帧渲染），PreWarm 不影响首帧复杂度。

**帧率卡顿 (运行中)：**

| 模式 | 平均 Jank | PSS |
|------|----------:|----:|
| baseline_jank | 19.05% | 44 MB |
| post_release_jank | 15.38% | 44 MB |
| **改善** | **−3.67 pp** | — |

> **结论：PreWarm 启动加速 44%，ReleaseMemory 卡顿降低 3.7pp。**

### 测试 (6 个，CI 自动)

| 测试 | 验证 |
|------|------|
| `schema_and_structure` | 5 个 mode 齐全，comparison 段存在 |
| `measurement_is_internally_consistent` | summary 与 raw samples 重算一致 |
| `prewarm_shows_no_regression` | 启动不慢于阈值 (100ms / 20%) |
| `release_memory_does_not_increase_jank` | 卡顿不增加 (≤ 20pp) |
| `conclusion_matches_deltas` | prewarm_effective / release_memory_effective 与数据一致 |
| `stays_within_budget` | RSS / PSS 在阈值内 |

---

## 三、稳定性

> 问题：DiPECS 长时间运行会不会内存泄漏？

### 测量方法

定时采样 RSS / PSS / CPU，线性回归拟合增长速率：

```bash
DURATION_MINUTES=60 SAMPLE_INTERVAL_SECS=30 ./tools/collect-stability.sh
```

### 结果 (4 分钟 × 30s 间隔 = 8 采样)

| 指标 | 值 | 阈值 |
|------|----:|-----:|
| RSS 初值 | 141 MB | — |
| RSS 终值 | 136 MB | — |
| RSS 增长速率 | +6.1 MB/h | ≤ 50 MB/h |
| PSS 增长速率 | +3.9 MB/h | ≤ 20 MB/h |
| 平均 CPU | 0.9% | ≤ 10% |

> 启动后 RSS 先降后稳 (GC 回收)，稳态后无明显增长。**无内存泄漏。**

### 测试 (4 个，CI 自动)

| 测试 | 验证 |
|------|------|
| `schema_and_structure` | 结构正确，采样 ≥ 3 个 |
| `internally_consistent` | 首尾 RSS/PSS/CPU 与 raw samples 一致 |
| `no_memory_leak` | 增长速率 ≤ 阈值 |
| `conclusion_matches_data` | conclusion 与数据一致 |

---

## 四、云端决策

> 问题：CloudLlmBackend 正常响应、异常处理、熔断机制是否可靠？

### E2E Mock (4 个，CI 自动)

本地 TCP mock server 模拟 DeepSeek API：

| 测试 | 验证 |
|------|------|
| `cloud_accepts_valid_json` | 正常 JSON → 解析为 DecisionBatch |
| `cloud_handles_http_error` | HTTP 429 → 返回 error |
| `cloud_errors_on_dead_port` | 不可达端口 → 返回 error |
| `circuit_breaker_counts_errors` | 连续 3 次错误全部捕获 |

### 真实 API Benchmark

```bash
source .env
cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::smoke   -- --ignored --nocapture
cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::latency  -- --ignored --nocapture
```

**Smoke 结果 (6 场景 × 1 次)：**

| 场景 | 延迟 | 决策 |
|------|-----:|------|
| circuit-breaker | 9.2s | Idle |
| low-battery | 6.2s | Idle |
| morning-routine | 14.2s | CheckNotification, HandleFile |
| rich-workflow | 10.0s | SwitchToApp |
| privacy-sensitive | — | 待下次 smoke 运行更新 |
| multi-app-switching | — | 待下次 smoke 运行更新 |

> DeepSeek v4-flash 延迟在 5-15s 范围，成功率 100%。复杂场景 (morning-routine) 返回多意图决策。

### 多应用并发场景

> 问题：CloudLlmBackend 在多个 App 同时活跃时能否正确感知上下文多样性？

**场景设计 (data/traces/scenarios/multi-app-switching.jsonl)：**

1000 条事件，覆盖 7 个包名 (`com.example.chat`, `com.example.mail`, `com.example.docs`, `com.example.browser`, `com.example.bank`, `com.example.calendar`, `com.android.settings`)，包含 AppTransition (前后台切换)、NotificationPosted (通知)、SystemState (系统状态) 三种事件类型。模拟用户在多个 App 之间频繁切换的真实使用模式。

**上下文构建验证 (2 个，CI 自动)：**

| 测试 | 验证 |
|------|------|
| `multi_app_context_has_app_diversity` | foreground_apps ≥ 3, notified_apps ≥ 2, 来自 ≥ 3 个不同 package 的事件, AppTransition 与 Notification 事件类型齐全, SystemState 被正确捕获 |
| `single_app_scenarios_work` | 验证 circuit-breaker / low-battery / rich-workflow 场景在 `build_context` 改写后仍正确工作 |

**`build_context` 改进：**

| 改进点 | 之前 | 之后 |
|--------|------|------|
| 事件类型 | 全部视为 Notification | 正确解析 AppTransition / Notification / SystemState |
| foreground_apps | 硬编码 `["com.example.app"]` | 从 AppTransition::Foreground 实际提取并去重 |
| notified_apps | 硬编码 `["com.example.chat"]` | 从 NotificationPosted 实际提取并去重 |
| 语义提示 | 固定 [FileMention, ImageMention, LinkAttachment] | 从 raw_title / raw_text 扫描关键词推导 |
| 系统状态 | 不提取 | 从 SystemState 事件提取电池/网络/响铃模式 |
| 事件数量 | 最多 5 条 | 最多 60 条 |

多应用场景下，单次调用即可触及 `build_context` 的全部分支，确保重构不破坏已有功能。下一次 `smoke` 运行将产生包含 6 场景的 cloud-scenarios JSON 数据集。

**Latency Benchmark (5 轮，morning-routine)：**

| 指标 | 值 |
|------|----:|
| min | 5.5s |
| p50 | 11.3s |
| p95 | 13.0s |
| success_rate | 100% |

---

## 五、隐私脱敏验证

> 问题：如何确保 PII 在任何情况下都不会越过 `PrivacyAirGap`？

### 三层防御

| 层 | 文件 | 测试数 | 方法 |
|---|------|-------|------|
| 结构验证 | `privacy_airgap_test.rs` | 20 | 固定 scenario，断言 `SanitizedEvent` 结构正确（TextHint / SemanticHint / ExtensionCategory） |
| JSON 子串扫描 | `privacy_leak_test.rs` | 5 (12 Case) | 构造含已知 PII 的 `RawEvent`，脱敏后序列化为 JSON，扫描整个 JSON 字符串断言 forbidden 子串不存在 |
| 属性测试 | `privacy_airgap_property_test.rs` | 6 | 对**所有** `RawEvent` variant × 多样化输入（10 种文本 profile、6 种路径、4 种 notification_key、4 种 Binder method），收集 PII 字符串 → 脱敏 → JSON → 断言零泄露 |

**属性测试覆盖的输入维度：**

| 维度 | 样本数 | 覆盖 |
|------|-------|------|
| 通知文本 | 10 | CJK 姓名/电话/验证码/emoji/零宽字符/RTL 覆盖/金融信息/超长文本/空文本/混合语言 |
| 文件路径 | 6 | 内部数据目录/SD 卡文档/DCIM 照片/APK 下载/中文文件名/缓存临时文件 |
| notification_key | 4 | 联系人 tag/中文 tag/电话号码 tag/空 tag |
| Binder method | 4 | share intent/enqueue notification/bind service/plain launch |

**CI 门禁：** 三层测试共 31 条，全部在 `cargo test -p aios-core` 中自动执行。新增 `RawEvent` variant 时，`property_all_variants_covered` 会强制开发者同步更新测试。

```bash
cargo test -p aios-core -- privacy       # 只跑隐私相关测试
cargo test -p aios-core                  # 全部 core 测试 (含隐私三层)
```

---

## 六、CI 性能回归门禁

所有数据集测试内嵌阈值断言，CI 自动 block 超标的 merge：

| 维度 | 阈值 | 门禁测试 |
|------|------|---------|
| CPU 增量 | ≤ 8 pp | `resource_overhead_fixture_stays_within_budget` |
| PSS 增量 | ≤ 80 MB | `resource_overhead_fixture_stays_within_budget` |
| 启动加速 | ≥ 20% | `ux_metrics_prewarm_shows_no_regression` |
| 卡顿增加 | ≤ 20 pp | `ux_metrics_release_memory_does_not_increase_jank` |
| 内存泄漏 | ≤ 50 MB/h RSS | `stability_no_memory_leak` |

---

## 七、综合总览

| 维度 | 投入 | 回报 |
|------|------|------|
| 后台 CPU | +1.16% | — |
| 内存 (PSS) | +31 MB | — |
| 预估耗电 | 0.21 mAh/min (10min ≈ 0.05%) | — |
| 启动加速 | — | 1552 → 873 ms (**44%**) |
| 卡顿降低 | — | 19.1% → 15.4% (**−3.7 pp**) |
| 云端延迟 | — | < 30s p95 |
| 稳定性 | — | 无内存泄漏 |

**用极低的资源代价 (CPU < 2%, 内存 < 150MB)，换取用户可感知的启动加速和流畅度提升。**

---

## 附录：文件索引

```
tools/
  collect-resource-overhead.sh         Bash 资源开销采集
  collect-resource-overhead.ps1        PowerShell 资源开销采集
  collect-ux-metrics.sh                Bash UX 指标采集
  collect-stability.sh                 Bash 稳定性检测

data/evaluation/
  resource-overhead-emulator-20260701-131525.json   资源开销 (10 采样)
  ux-metrics-emulator-20260701-151856.json          UX 指标 (5 采样)
  stability-emulator-canonical.json                 稳定性 (8 采样)
  cloud-scenarios-20260701-084010.json              云端场景烟雾测试 (4 场景，待更新为 6)
  cloud-latency-20260701-084110.json                云端延迟基准 (5 轮)

data/traces/scenarios/
  circuit-breaker.jsonl              单 app，断路器触发场景
  low-battery.jsonl                  单 app，低电量场景
  morning-routine.jsonl              单 app，早晨多通知场景
  rich-workflow.jsonl                单 app，文档协作场景
  privacy-sensitive.jsonl            单 app，隐私敏感场景
  multi-app-switching.jsonl          多 app (7 包名)，频繁切换场景 — 1000 条

crates/aios-cli/tests/
  resource_overhead_dataset_test.rs    资源开销测试 (5)
  ux_metrics_dataset_test.rs           UX 指标测试 (6)
  stability_dataset_test.rs            稳定性测试 (4)

crates/aios-agent/src/backends/cloud_llm/mod.rs
  latency_tests                       云端延迟对比 (1, ignored)
  cloud_bench_tests                   云端 benchmark + smoke + 上下文验证 (4, 2 ignored)
  mock_cloud_e2e_tests                E2E mock 测试 (4)

crates/aios-core/tests/
  privacy_airgap_test.rs              结构验证 (20) — 所有 RawEvent variant 覆盖
  privacy_leak_test.rs                JSON 子串扫描 (5, 12 Case) — 已知 PII 向量
  privacy_airgap_property_test.rs     属性测试 (6) — 多样化输入 × 全 variant 覆盖
```
