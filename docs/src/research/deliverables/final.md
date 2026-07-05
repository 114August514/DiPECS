# 结题报告：最终成果展示

> 对应学术交付：结题报告 PDF。本文档反映当前仓库实现状态，包含截至
> 2026-07-05 的 Pixel 6a 真机动作收益证据。

## 项目概览

DiPECS 在 Android 平台上实现了一条本地优先的 AIOS 原型链路：

```text
Android public API / daemon source
    -> CollectorEnvelope / RawEvent
    -> PrivacyAirGap
    -> StructuredContext
    -> DecisionRouter
    -> PolicyEngine
    -> AuthorizedAction
    -> Android action bridge / local executor trace
```

当前实现的核心价值是：把 Android 本地事件转化为窗口级上下文状态，再把预测转化为可测的资源动作收益。隐私、策略和审计是这条动作链路的安全前提，而不是替代性能收益的成果本身。

## 核心成果

### 采集能力

已落地的生产入口：

- `UsageStatsManager` -> `RawEvent::AppTransition`
- `NotificationListenerService` -> `RawEvent::NotificationPosted` / `RawEvent::NotificationInteraction`
- `DeviceContext` -> `RawEvent::SystemState`
- Android append-only JSONL -> `dipecsd --android-trace-jsonl`

仍作为筛选/增强来源：

- `AccessibilityService`：app 侧可记录和预览，但没有 Rust schema 的行以 `rawEvent: null` 表示，生产 Rust ingress 会跳过。

系统侧预留：

- `/proc`、Binder probe、fanotify/system image 路线保留在 spec/设计中，作为后续更高权限部署能力。

### 推理与策略

- `aios-agent` 已提供 `DecisionRouter`、`RuleBasedBackend`、`LocalEvaluatorBackend`、`CloudLlmBackend`、`FallbackNoOpBackend`。
- Cloud LLM 支持 DeepSeek、Qwen/DashScope 和 OpenAI-compatible endpoint。
- `PolicyEngine` 使用 `CapabilityLevel`、风险等级、置信度、目标上下文和动作 allow-list 做最终授权。
- Cloud LLM 错误不会记录 HTTP 响应体，避免 provider 错误信息进入日志/audit 链路。

### 动作与 Android Bridge

- `aios-action` 保留本地 replay fallback。
- `PrefetchFile(url:/uri:)` 可通过 Android localhost bridge 转发给 Android collector，并在 Pixel 6a 上完成 #97 真机收益 gate。
- `ReleaseMemory cache:volatile` 可释放由 `PreWarmProcess own:volatile-cache:<MB>` seed 的 app-owned 可丢弃内存，并在 Pixel 6a 真压力场景下完成 #99 gate。
- Android action socket 使用 `auth_token` 鉴权，token 存储在 `EncryptedSharedPreferences`。
- socket 读取具备 payload 大小限制、读超时、失败退避和调度失败记录。

### Trace 与回归

- `aios-cli replay` 可回放 Android JSONL，并输出 ingest/sanitize/intent/policy/action/summary NDJSON。
- `RuntimeTraceRecorder` 可记录 daemon 窗口级运行 trace。
- CI replay 使用 sample 和 denial trace 生成 output/audit artifact。

## 评测与验证状态

已通过的本地验证：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
git diff --check
```

Android Gradle 构建依赖本机 Android SDK；当前本地未配置 `ANDROID_HOME`，因此 Android APK 构建主要依赖 GitHub Actions 验证。

## 性能与质量指标

| 指标 | 目标 | 当前状态 |
| :--- | :--- | :--- |
| Rust 测试 | 全部通过 | `cargo test --workspace` 通过 |
| Rust lint | 无 warning | `cargo clippy ... -D warnings` 通过 |
| 格式检查 | 稳定 | `cargo fmt --all -- --check` 通过 |
| Golden / replay | 稳定可回放 | sample、denial、privacy leak 测试已覆盖 |
| Android ingress | 可进入 daemon/core | JSONL tailer + replay tests 覆盖 |
| Android APK | CI 验证 | 需要 GitHub Actions / Android SDK |
| 真机 Android | 已纳入核心动作收益证据 | Pixel 6a 完成 PreWarm、PrefetchFile、ReleaseMemory `cache:volatile` 的 n>=20 gate；KeepAlive 机制边界已降级 |

## 真机动作收益证据

当前正面收益只按动作机制逐项引用，不能合并成笼统的“真实场景体验显著改善”。

| 动作 | 真机证据 | 结论边界 |
| :--- | :--- | :--- |
| `PreWarmProcess own:*` | Pixel 6a n=20/mode：cold mean/p95 710.75/733 ms，prewarm hit mean/p95 201.55/213 ms；LSApp standard 投影 DiPECS `net_benefit_ms=76,068,875.158`，强基线 `72,283,770.198` | 关闭 #90 的 Android-safe 自有资源预热 gate；不声称普通 APK 可静默预热第三方应用 |
| `PrefetchFile` | Pixel 6a #97 n=20/mode：399,165-byte HTTPS 目标，prefetched read mean/p95 79.993/101.332 ms，miss fetch+read mean/p95 1860.332/2276.297 ms；投影 DiPECS `61,268,324.531 ms` > 强基线 `34,859,928.678 ms` | 设备直接测每次命中节省和 miss 成本；DiPECS-vs-strong 来自 LSApp hit@1 投影 |
| `ReleaseMemory cache:volatile` | Pixel 6a #99 n=20/mode：512 MB 非 root 匿名内存压力 + 64 MB app-owned volatile cache；available gain +55,158.6 KB，PSS reduction gain +64,621.3 KB，Welch p=0.00026891，jank 0.0 pp | 正面结果仅覆盖 app-owned volatile memory release；旧 `cache:prefetch` 磁盘缓存清理不得作为内存收益引用 |
| `KeepAlive` | 真机/模拟器 app 形态下 `oom=denied,cgroup=denied`；root 代写 `oom_score_adj` 也会被 AMS 覆盖 | 普通 app 形态无法证明抗杀收益；需 platform-signed `/system/bin/dipecsd` 才可能验证 |

## 创新点

1. **Measured Android resource actions**：PreWarmProcess、PrefetchFile 与 ReleaseMemory `cache:volatile` 在 Pixel 6a 上给出 n>=20 动作级收益证据。
2. **Window-level context state**：Android public API、设备状态与 `/proc` 进程状态被聚合为 `StructuredContext`，避免把噪声事件直接交给决策器。
3. **Mechanism-Policy Separation**：LLM/规则只提出意图，动作是否执行由本地 `PolicyEngine` 决定。
4. **Privacy Air-Gap**：原始 PII 在本地脱敏边界内被截断，推理层只看到结构化摘要。
5. **Deterministic Trace Replay**：Android JSONL 可以被 `aios-cli replay` 稳定回放，支持 audit hash、privacy leak 和 denial golden 测试。

## 局限与未来工作

- 长期用户体验、真实用户 field study 和真机功耗仍未完成，不能从动作级 gate 外推为整体 UX 改善。
- `KeepAlive` 在普通 Android app 形态下机制不成立，后续需要 platform-signed `dipecsd` 或 system image 集成。
- 系统态采集路线（fanotify、Binder/eBPF、system image）还未作为主线部署。
- `ReleaseMemory cache:volatile` 是语义升级后的正面结果；旧 `cache:prefetch` 删除磁盘文件的负面结果必须保留为边界说明。

## 相关文档

- [v0.2 发布说明](../../rfc/releases/v0.2.md)
- [Daemon 架构设计](../../architecture/pipeline.md)
- [Android 接口 MVP](../../android/collector.md)
- [Android action boundary](../../android/action-boundary.md)
- [RFC-0001](../../rfc/0001-layered-collection-and-decision-routing.md)
