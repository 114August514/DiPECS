---
hide:
  - toc
---

# DiPECS 文档中心

DiPECS（Digital Intelligence Platform for Efficient Computing Systems）是一个运行在 Android 设备上的意图驱动 AIOS 原型，围绕 **本地采集 → 脱敏整理 → 云端 LLM 决策 → 本地执行 → 能力自迭代** 的闭环设计。

---

## 开始阅读

<div class="grid cards" markdown>

-   :material-book-open-page-variant:{ .lg .middle } __核心架构指南__

    ---

    系统设计、状态机、模块边界、RFC 提案、开发规范。

    [:octicons-arrow-right-24: 进入指南](design/overview.md)

-   :material-language-rust:{ .lg .middle } __Rust API 参考__

    ---

    `cargo doc` 自动生成，覆盖 `aios-spec` / `aios-core` / `aios-agent` / `aios-action` / `aios-daemon`。

    [:octicons-arrow-right-24: 打开 API 文档](https://114august514.github.io/DiPECS/api/)

-   :material-file-document-multiple:{ .lg .middle } __学术材料__

    ---

    当前 Markdown 学术交付、未来正式 PDF 报告、答辩材料入口。

    [:octicons-arrow-right-24: 浏览材料](academic/index.md)

</div>

<!-- ACADEMIC_REPORTS_PLACEHOLDER -->

---

## 技术路线

| 层级 | 语言 | 职责 |
| :--- | :--- | :--- |
| Android 应用层 | Kotlin | 权限、服务、UI、行为/上下文采集、优化动作执行 |
| 核心逻辑层 | Rust | 事件模型、上下文聚合、脱敏、策略校验、Trace 回放 |
| 云端判断层 | Remote LLM + Skills | 场景理解、Skill 选择与调用、置信度判断、Skill 自迭代 |

## 历史版本

<!-- ARCHIVE_LIST_PLACEHOLDER -->

<!-- BUILD_TIMESTAMP_PLACEHOLDER -->
