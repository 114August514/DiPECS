# 结题报告：最终成果展示

> 对应学术交付：结题报告 PDF。本文档反映当前仓库实现状态，不包含真机 Android 验证结果。

## 项目概览

DiPECS 在 Android/Linux 场景下实现了一条本地优先的 AIOS 原型链路：

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

当前实现的核心边界是：原始事件只存在于 collector 到 `PrivacyAirGap` 的短路径上；推理层只接收脱敏后的 `StructuredContext`；动作层只执行经过策略审查的 `AuthorizedAction`。

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
- `PrefetchFile(url:/uri:)` 可通过 Android localhost bridge 转发给 Android collector。
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
| 真机 Android | 待验证 | 尚未纳入本文结果 |

## 创新点

1. **Privacy Air-Gap**：原始 PII 在本地脱敏边界内被截断，推理层只看到结构化摘要。
2. **Mechanism-Policy Separation**：LLM/规则只提出意图，动作是否执行由本地 `PolicyEngine` 决定。
3. **Android public-API production ingress**：把 UsageStats、NotificationListener、DeviceContext 提升为可进入 Rust daemon 的生产入口，同时保留 Accessibility 作为筛选来源。
4. **Deterministic Trace Replay**：Android JSONL 可以被 `aios-cli replay` 稳定回放，支持 audit hash、privacy leak 和 denial golden 测试。
5. **Authenticated Action Bridge**：Android localhost action socket 通过 token 鉴权、限速、超时和 payload 限制降低本地跨应用注入风险。

## 局限与未来工作

- 真机 Android 验证尚未完成，包括权限授予、trace 导出、adb forward、action bridge 和 APK 安装路径。
- `PreWarmProcess`、`KeepAlive`、`ReleaseMemory` 当前仍以本地 fallback/trace 为主，后续应收敛为 Android-safe 的自有资源动作。
- 系统态采集路线（fanotify、Binder/eBPF、system image）还未作为主线部署。
- 最终报告需要补入真实设备运行截图、CI artifact 链接和真机 trace 样本。

## 相关文档

- [v0.2 发布说明](../../rfc/releases/v0.2.md)
- [Daemon 架构设计](../../architecture/pipeline.md)
- [Android 接口 MVP](../../android/collector.md)
- [Android action boundary](../../android/action-boundary.md)
- [RFC-0001](../../rfc/0001-layered-collection-and-decision-routing.md)
