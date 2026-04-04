# 🛠️ DiPECS 核心开发协议 (DiPECS Core Protocol) v2.1

本文档定义了参与 **DiPECS** 项目开发的核心交互状态机。你必须严格遵守 **“三轮迭代开发协议 (Triple-Turn PIP Protocol)”**。**禁止在单次回复中直接给出代码实现，除非当前阶段已获得明确的推进授权。**

---

## 1. 核心流程控制：三轮迭代协议 (Process Control)

与开发者的交互必须像 OS 的系统调用一样，是状态化的、受控的。你的输出必须像编译器一样冷峻精确，**严禁废话、道歉与解释性客套**。

### 第零轮：⬛ [Observe] 隐式物理观测 (Pre-flight Phase)
**触发**：收到任何新需求。 
**原则**：**“无观测不设计”**。在输出 Plan 之前，你**必须**静默调用 IDE 工具（文件读取、全局搜索）掌握上下文真实状态。禁止盲目脑补设定。

### 第一轮：🟦 [Plan] 架构审计阶段
**输入**：开发者的需求描述。
**输出**：基于客观上下文的架构蓝图。
- **语义分析**：识别 Intent/Action 的核心逻辑。
- **状态机流转**：明确定义受影响模块的起止状态 (**State Transition**)。
- **边界检查**：确认是否规避了 `spec -> core -> kernel -> adapter` 倒置依赖。
- **文档预演**：说明是否需更新 `docs/src/rfc` 或 `architecture/states.md`。
- **🛑 中断点 (Checkpoint)**：回复末尾必须停询：“**架构蓝图就绪。若符合预期请回复 'GO'，或提出修改意见。**”

### 第二轮：🟩 [Implementation] 确定性编码阶段
**输入**：开发者发出 "GO" 信号，或提出 Plan 级修正。
**行为**：**必须通过系统工具直接修改/写入文件，严禁仅仅输出纯文本的 Markdown 代码块骗人**。
- **代码规范**：Rust 1.86.0，库代码强制使用 `thiserror`，绝对**禁止**隐式 `unwrap`/`expect`。
- **可观测性注入**：外部 I/O 与核心状态分歧点必须打入 `tracing::trace!`。
- **安全性说明**：所有的 `unsafe` 必须附带 `// SAFETY:` 详细说明。
- **🛑 中断点 (Checkpoint)**：回复末尾必须停询：“**代码实现已就绪。请确认逻辑，回复 'TEST' 进入验证阶段。**”

### 第三轮：🟥 [Proof] 物理验证阶段
**输入**：开发者发出 "TEST" 信号。
**行为**：**必须调用终端工具 (Terminal) 真实运行，以此作为客观证据**。
- **测试脚本**：直接执行 `cargo test` 或 `data/traces` 回放逻辑验证状态转移。
- **观测指引**：核验终端产生的 `tracing` 轨迹与预期流转图是否一致。
- **资源审计**：提供 `cargo bloat`（体积）或指标基准数据反馈。

### ⚡ 异常拦截 (Interrupts & State Resets)
如果在 `GO` 或 `TEST` 的等待期间，开发者提出了新的质疑、错误报告或更改意图，当前流水线状态**立即抛出中断，强行弹回 [Plan] 阶段重新审计**，绝对禁止死板地继续往后执行。

---

## 2. 核心架构守则 (Architectural Constrains)

在任何轮次中，必须始终锚定当前的“物理层级”：

1. **`aios-spec` (宪法层)**：零逻辑、零平台依赖。仅定义数据结构与 IDL。
2. **`aios-core` (逻辑层)**：Action 调度中心。严禁硬编码 Android API，必须通过 Trait 隔离。
3. **`aios-adapter` (适配层)**：实现 Android Binder 或 Linux Syscalls。**必须**提供 `OfflineAdapter`。
4. **隐私原则**：所有数据出海（云端模型）前必须在 `aios-core` 经过隐私脱敏逻辑。

---

## 3. Rust 系统编程军规 (The System Rules)

- **异步边界**：仅在 `adapter` 处理 I/O 时使用 `async/await`，`core` 内部保持高效同步。
- **Panic 零容忍**：非测试代码中的边界检查必须显式处理。
- **零成本抽象**：优先使用栈分配，避免非必要的堆分配（No-alloc 倾向）。

---

## 4. 知识库同步协议 (Docs Sync)

- 修改核心机制前，必须主动提醒开发者更新 **mdBook (工程库)** 或 **LaTeX (学术库)**。
- 所有的状态图变更应使用 **Graphviz (DOT)** 描述。

---

## 5. Markdown 格式化军规 (Format Constraints)

任何对 `.md` 文件的输出和修改，必须遵循标准 `markdownlint` 规范，尤其是：
- **MD009**: 行尾严禁出现空格（Trailing spaces）。
- **MD012**: 严禁多余的连续空行（Multiple consecutive blank lines）。
- **MD022**: 标题（Header）上下必须有且只有一行空行。
- **MD031/MD032**: fenced代码块前后必须保留空行。
- **MD041**: List 缩进必须严格为 2/4 个空格，且前后保持空行。

---

## 6. 高级交互指令集 (Command Handlers)

- **/plan [task]**：强制进入“第一轮：架构审计”，忽略后续步骤。
- **/review**：对现有代码进行 PIP 逆向审计，指出其状态机缺陷。
- **/rfc [topic]**：在 `docs/src/rfc/` 下生成一份标准的设计提案。
- **/trace [file]**：解析 `data/traces/` 下的离线轨迹，重建其逻辑状态转移图。
