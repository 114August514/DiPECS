---
hide:
  - toc
---

# DiPECS 文档中心

DiPECS（Digital Intelligence Platform for Efficient Computing Systems）当前是一个 Android/Linux AIOS 原型系统。它的主线不是“让模型直接控制设备”，而是把本地信号采集、隐私脱敏、上下文聚合、决策路由、策略审查和授权动作执行拆成可审计的边界。

当前默认闭环是：

```text
Android / daemon / replay sources
  -> RawEvent
  -> PrivacyAirGap
  -> StructuredContext
  -> DecisionRouter
  -> IntentBatch
  -> PolicyEngine
  -> ActionLifecycle
  -> AuditRecord
```

Cloud LLM 是可选后端；默认路径由本地 `RuleBasedBackend` 和保守 fallback 支撑。

---

## 开始阅读

<div class="grid cards" markdown>

-   :material-map-marker-path:{ .lg .middle } __当前实现__

    ---

    与源码对齐的运行链路、数据流、动作治理、Android 桥接和 replay/audit。

    [:octicons-arrow-right-24: 查看当前实现](current/overview.md)

-   :material-book-open-page-variant:{ .lg .middle } __系统设计__

    ---

    架构原则、模块边界、代码地图、RFC 和历史设计说明。

    [:octicons-arrow-right-24: 进入系统设计](design/overview.md)

-   :material-language-rust:{ .lg .middle } __Rust API 参考__

    ---

    `cargo doc` 自动生成，覆盖 workspace 内 Rust crates。

    [:octicons-arrow-right-24: 打开 API 文档](https://114august514.github.io/DiPECS/api/)

-   :material-file-document-multiple:{ .lg .middle } __学术与历史材料__

    ---

    课程交付、调研、答辩材料和历史会议纪要。这里的表述可能反映当时状态。

    [:octicons-arrow-right-24: 浏览材料](academic/index.md)

</div>

<!-- ACADEMIC_REPORTS_PLACEHOLDER -->

---

## 当前模块

| 层级 | 模块 | 职责 |
| :--- | :--- | :--- |
| Android 应用层 | `apps/android-collector` | 公开 API 采集、JSONL trace、action socket、Android-safe actions |
| 协议层 | `aios-spec` | 跨 crate 数据结构与治理协议 |
| 采集层 | `aios-collector` | Android JSONL tailer、daemon/system source 入口 |
| 核心层 | `aios-core` | 隐私脱敏、窗口聚合、策略审查、动作生命周期 |
| 决策层 | `aios-agent` | Rule-based / Cloud LLM / fallback 决策后端 |
| 执行层 | `aios-action` | `ActionAdapter`、Android bridge forwarding、offline adapter |
| 运行时 | `aios-daemon` | `dipecsd` 两 task 在线管线 |
| 工具层 | `aios-cli` | replay、audit hash、Android socket ping |

## 历史版本

<!-- ARCHIVE_LIST_PLACEHOLDER -->

<!-- BUILD_TIMESTAMP_PLACEHOLDER -->
