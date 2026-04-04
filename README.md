# 🚀 DiPECS (Digital Intelligence Platform for Efficient Computing Systems)

<!-- Badges Area: CI Status, Rust Version, NDK Version, License -->
![CI Status](https://img.shields.io/badge/CI-Pipeline-success)
[![Rust Version](https://img.shields.io/badge/Rust-1.86.0-orange)](rust-toolchain.toml)
[![NDK Version](https://img.shields.io/badge/NDK-r27d-green)](scripts/setup-env.sh)
[![License](https://img.shields.io/badge/License-Apache--2.0-blue)](LICENSE)

## 📖 1. 项目愿景 (Vision)

> **"The brain is in the cloud, but the reflexes must be local."**

DiPECS 是一款基于 **云端大模型驱动 (Cloud-LLM Driven)** 的下一代分布式意图操作系统原型。它通过严格的 **“机制与策略分离”**，在 Android (API 33) 物理环境下，实现用户意图感知、隐私安全脱敏、云端决策推断与本地确定性执行的高效闭环。

本项目深受蒋炎岩 (JYY) 老师的操作系统哲学启发，在研发规范上坚持对 **状态机流转确定性**、**全链路可观测性** 及 **极致的工程严谨** 提出最高要求。

## 🏗️ 2. 系统架构 (Architecture)

### 2.1 逻辑抽象视图

DiPECS 展现了由意图 (Intent) 降维到物理操作 (Action) 的状态机流动管道：

```mermaid
graph TD
    User((User Intent)) --> Experience[Experience Layer / CLI]
    Experience --> Agent[aios-agent: Semantic Orchestration]
    Agent -- "Encrypted/Redacted" --> Cloud((Cloud LLM))
    Cloud -- "Action Schema" --> Core[aios-core: Action Bus & Policy Engine]
    Core --> Kernel[aios-kernel: Resource Management]
    Kernel --> Adapter[aios-adapter: Android/Linux Syscalls]
    Adapter --> HW[[Physical Hardware / Android API 33]]
```

### 2.2 机制与策略分离设计

云端大脑主管“策略”逻辑（推演意图、结构化提取），而本地端严格聚焦“机制”（隐私清理、路由分发、硬件鉴权调用）。全系统严格遵守 `spec -> core -> kernel -> adapter` 的单向不可逆依赖栈流，拒绝越级抽象。

## 📦 3. 车间布局 (Project Structure / Workspace)

整个 DiPECS 被划分为 6 个权责分明的 Crate 工作区，依赖界限物理隔离：

| Crate | 层级 | 职责 | 依赖约束 |
| :--- | :--- | :--- | :--- |
| **`aios-spec`** | **宪法层** | 系统 Single Source of Truth。收束所有数据结构、Trait 及 Action Schema 接口。 | **零内部依赖**，绝对隔离 |
| **`aios-core`** | **逻辑层** | Action Bus 的枢纽。统筹意图调度、负责 **Privacy Air-gap (隐私物理隔绝)** 与防渗漏审计。 | `aios-spec` |
| **`aios-kernel`** | **内核层** | 控制底层资源生命周期、任务时序分配与确定性进程协同（IPC）。 | `aios-core`, `aios-spec` |
| **`aios-adapter`** | **适配层** | 跨接纯逻辑层与物理世界。代理 Android Binder 与 Linux Syscalls。 | `aios-kernel`, `aios-spec` |
| **`aios-agent`** | **业务层** | 充当 LLM 大脑本地 Proxy。执行语义编组拆解与对话轮次上下文控制。 | （最高应用层）隔离 |
| **`aios-cli`** | **工具层** | 提供交互沙盒与系统状态的“显微镜”探针 (Observability TUI)。 | - |

## 🛠️ 4. 环境引导与自举 (Bootstrap & Toolchain)

针对跨 Android/Linux AArch64 繁琐的配置，架构组通过自动化脚本抹平了环境门槛。

### 4.1 预备环境 (Prerequisites)

一台类 Unix 系统机器，依赖原生编译宿主环境。

### 4.2 一键初始化 (Setup)

注入 Rust 1.86.0 与预编译的 Android NDK (r27d) 环境：

```bash
source scripts/setup-env.sh
```

### 4.3 交叉编译与部署 (Cross-Compilation & Deploy)

```bash
# 对 AArch64 Android 环境生产构建
cargo android-release

# 推送临时产物至 Android 设备 (/data/local/tmp) 并挂载执行
./scripts/android-runner.sh
```

## 🤖 5. AI 协作协议 (The PIP Protocol v2.1)

本项目引入高度特化的内建大模型开发流水线。无论是人类 Dev 还是 AI Agent，必须遵从 **`.github/copilot-instructions.md`** 中的 **“三轮迭代状态机 (Triple-Turn PIP Protocol)”** 指导闭环开发。

### 5.1 三轮迭代状态机

- **🟦 1. [Plan] 架构审计**：识别需求 Intent，绘制核心 State Transition 边界，拦截不合理的单向物理层穿透。通过 `GO` 获准推进。
- **🟩 2. [Implementation] 确定性编码**：0 Panic 容忍（禁 `unwrap/expect`），必须抛出强类型 `thiserror` 错误，注入 Trace 观测点。以 `TEST` 作为检验放行标准。
- **🟥 3. [Proof] 物理验证**：终端真实触发离线轨迹或 Cargo 单元拦截测试作为强力验证锚点。

### 5.2 隐式观测规则

**"无观测不设计" (⬛ [Observe] 阶段)**。禁止盲目脑部猜想上下文拓扑配置。一切架构干涉与实现重构前，要求必定进行工具静默查探。

## 📚 6. 双轨知识库 (Documentation)

团队维持极度苛刻的双轨并行知识基操，覆盖敏捷协作与院系报备双向要求：

- **📖 工程指南 (mdBook)**: `docs/src/`。供项目组共享 RFC 提案、API 文档以及系统的状态转移模型图 (`architecture/states.md`)。
- **🎓 学术报告 (LaTeX)**: `docs/academic/`。承载科研产出，从早期的课题可行性探讨 (`Survey`) 到中后期汇报自动化出刊体系。

## 🧪 7. 质量堡垒与可观测性 (CI/CD & Observability)

> **"No observation, no debugging."**

*通过 `public/index.html` 访问自动生成的全量系统视图（包含实时更新的状态转移图与 API 拓扑）。*

### 7.1 本地守卫 (Git Hooks)

所有 Push 前需历经大满贯本地筛查防线：

```bash
# 包含 fmt, clippy (零警告拦截), cargo tests 及基于 android target 交叉编译回归查验。
./scripts/check-all.sh
```

### 7.2 云端离线流水线 (Data Traces)

**任何 Logic Layer 的变更必须通过 Golden Traces 的 0 偏差校验（State Machine Replication）。**

使用 `data/traces/` 重建并固化物理世界的历史态调用流。当脱离 Android 真机或处于远端部署沙箱时，借助 `aios-adapter` 预装的 `OfflineAdapter` 进行确定性重放检验语义回归程度。

### 7.3 物理指标审计 (Bloat & Bench)

合流前必须审计的系统剖面：

- **体积 (bloat)**: 限制核心依赖的重型膨胀 (`deny.toml`)。
- **轨迹 (tracing)**: 将涉及文件 I/O、Action 派发调用的异步流精准挂载入微秒级拓扑监控图层中。

## 🤝 8. 贡献指南 (Contributing)

参与合并请遵循 [**CONTRIBUTING.md**](CONTRIBUTING.md)。基本素养：**Issue First** 且 严格践行 **PIP 协议回流机制**。

## 📜 9. 许可证 (License)

采用开放的 [**Apache License 2.0**](LICENSE) 授权发行。
