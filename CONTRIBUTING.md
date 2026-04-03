# 🛠 DiPECS 贡献指南

欢迎加入 **DiPECS (Digital Intelligence Platform for Efficient Computing Systems)** 项目团队。

**架构声明：** 本系统采用**云端大模型驱动**（Control Plane），本地环境则负责**环境感知、隐私脱敏、语义执行与状态反馈**（Data Plane）。本地 OS 不是算力中心，而是作为带有隐私隔离墙的“动作翻译官”和语义网关。

作为一个系统级开源项目，我们的核心原则是：**确定性 (Determinism)**、**可观测性 (Observability)** 和 **工程严谨性 (Rigor)**。请在开始编码前仔细阅读本规范。

---

## 1. 沟通与提案 (Issues & RFCs)

在开始编写大量代码之前，遵循“先讨论后执行”的原则：

- **Bug 修复**：请先在 Issue 板块搜索是否已有相关反馈，如果没有，请提交带有复现步骤的 Issue。
- **新功能/核心协议 (Spec) 变更**：请先提交 Issue 或 RFC (Request for Comments) 阐述动机和设计思路，获得内核维护者认可后再开始编码。

---

## 2. 环境一致性 (The "Ground Truth")

为了避免“在我的机器上能跑”这种非确定性故障，所有成员必须强制对齐：

- **Rust 版本**: `1.83.0` (由 `rust-toolchain.toml` 锁定)。
- **Android NDK**: `r27d` (必须安装并配置 `$ANDROID_NDK_HOME`)。
- **构建环境**: 任何标准的 **64-bit Linux** 环境（包括原生 Ubuntu 22.04+、Windows WSL2 或 Docker 容器）

**严禁**未经团队讨论私自升级或更改全局编译器版本。

---

## 3. 分支管理与代码审查

我们采用 **Feature Branch Workflow**。`main` 分支是神圣不可侵犯的“稳定状态”。

1. **创建分支**: 所有的开发必须在从 `main` 切出的功能分支上进行。
   - 命名规范: `feat/功能名` (如 `feat/action-bus`) 或 `fix/Bug名` (如 `fix/binder-leak`)。
2. **提交 PR**: 代码完成后，发起 Pull Request (PR) 指向 `main`。
   - **WIP (Work In Progress)**: 请使用 GitHub 的 "Convert to draft" 功能。维护者不会审计 Draft 状态的 PR。
   - **Ready for Review**: 只有当自检通过且标记为 Ready 时，才由 Owner 进行 Review。
3. **代码审计 (Code Review)**:
   - 每个 PR 必须至少经过 **2 名** 团队成员的 `Approve`。
   - 审计重点：状态机逻辑是否闭环？是否有内存泄露风险？协议 (Spec) 变更是否合理？
4. **合并**: 仅限 `Squash and Merge`，保持主干历史整洁。

---

## 4. 语义化日志：Commit Message 规范

我们采用 **Conventional Commits** 规范。良好的提交日志是系统演进的可搜索索引。

**格式**：`<type>(<scope>): <subject> (#Issue_ID)`
**要求**: 所有的 `feat` 和 `fix` 必须关联一个打开的 Issue。

- **feat**: 实现新功能 (例如: `feat(spec): define the payment action (#1)`)。
- **fix**: 修复 Bug (例如: `fix(core): resolve state machine deadlock (#42)`)。
- **docs**: 文档更新。
- **chore**: 构建流程或辅助工具变动（如修改 `.gitignore`）。
- **refactor**: 重构代码（不改变功能，不修复 Bug）。

---

## 5. 质量堡垒：自动化质检

在发起 PR 之前，**必须**在本地通过以下自检流程：

```bash
# 1. 格式化自检 (代码审美统一)
cargo fmt --all -- --check

# 2. 静态代码分析 (消除潜在 Bug)
cargo clippy --workspace -- -D warnings

# 3. 单元测试
cargo test --workspace

# 4. 安卓交叉编译验证 (确保不破坏移动端构建)
source scripts/setup-env.sh
cargo android-release
```

**CI 红了严禁合并。** 任何导致 `main` 分支无法编译的提交，责任人需在第一时间内修复或回滚。

### 5.5 资源约束自检 (Resource Constraints)

既然我们的目标是移动端边缘协同与高效调度，每一字节内存和每一毫秒延迟都至关重要：

- **体积检查**：运行 `cargo bloat --release --target aarch64-linux-android`。如果你的 PR 导致二进制体积异常增大（超过 100KB），必须提供合理性说明。
- **无分配 (No-alloc) 倾向**：在 `aios-core` 的热代码路径中，尽量避免频繁的 `Heap Allocation`，优先使用栈分配或预分配池。
- **性能红线 (Benchmarking)**：涉及调度算法、协议解析等核心路径的修改，必须运行 `cargo bench` (使用 `criterion`)。若导致吞吐量下降超过 5% 或延迟增加超过 1ms，需在 PR 中提供 Benchmark 对比报告并说明理由。

---

## 6. 协议变更与 RFC 流程

由于 `crates/aios-spec` 是全队的共同接口，任何对该模块的修改必须遵循 **RFC (Request For Comments)** 流程：

1. 在 `docs/rfc/` 下创建一个提案文档。
2. 在全队会议或群组中进行口头论证。
3. 达成共识后，方可修改 `aios-spec` 及其依赖项。

---

## 7. 模块分工与协作 (Architecture & Responsibilities)

我们在架构上追求**高内聚、低耦合**，每个模块职责单一，互不越界。团队成员应认领特定模块并成为其“Owner”：

- **`aios-spec` (协议层)**
  - **职责**: 定义系统的核心数据结构、Trait 和 IPC 协议。
  - **原则**: 零外部依赖，极度稳定。所有跨模块通信必须依赖此层的抽象，禁止跨模块的直接底层依赖。所有公开的 `struct` 和 `enum` 必须带有 `///` 文档注释，说明其物理含义与取值范围。
- **`aios-core` (核心引擎)**
  - **职责**: 实现任务调度器、工作流引擎、状态机管理。
  - **原则**: 不关心具体业务与平台，仅实现抽象通用逻辑。内部逻辑应尽可能保持同步 (Sync) 以保证性能与确定性。
- **`aios-kernel` (系统集成)**
  - **职责**: 负责初始化环境、模块串联组装、资源生命周期管理。
  - **原则**: 系统的核心胶水层，连接核心引擎与各种组件。处理具体的异步 (Async) I/O 等待。
- **`aios-adapter` (平台适配层)**
  - **职责**: 处理 Android/Linux 等平台差异，封装底层系统调用（如 IPC、网络、文件系统）。
  - **原则**: 依赖倒置，对上层隐藏平台细节，所有 OS 相关操作收敛于此。通过异步 (Async) 封装跨进程通信。
- **`aios-agent` / `aios-cli` (业务与工具层)**
  - **职责**: 具体 AI 代理的流程实现及命令行交互运维工具。
  - **原则**: 轻量级组装，纯业务逻辑，按需调用下层服务。

**依赖拓扑守则 (Dependency Graph)**：
为保证系统的可维护性与编译效率，必须严格遵守**单向依赖**原则：

- **核心屏障**：`aios-spec` 必须保持**零本项目内部依赖**。
- **层级流向**：推荐路径为 `spec` -> `core` -> `kernel` -> `adapter` -> `agent/cli`。
- **严禁循环**：严禁任何形式的循环依赖（如 `adapter` 依赖 `agent`）。若发现逻辑无法解耦，必须发起 RFC 重新审视模块边界。

**协作模式**：

- **隔离开发**：修改某模块代码时，需请该模块 Owner 强制 Review（取得 Approve 后方可合并）。
- **面向接口编程**：跨模块调用必须通过 `aios-spec` 定义的接口，严禁在 `aios-core` 中直接硬编码引入 `aios-adapter` 的具体实现。
- **依赖管理**：
  - 严禁随意引入第三方 Crate。
  - 引入任何新依赖必须在 PR 中明确说明理由：
    1. 必要性说明（为何不能用标准库或现有库）；
    2. 是否支持交叉编译（Android）；
    3. 对二进制体积的影响评估。

---

## 8. 开发者行为准则 (Code of Conduct)

我们希望构建一个专业、严谨且互相尊重的开源工程环境：

1. **就事论事，数据说话**：技术讨论应基于 Benchmark 性能数据、RFC 设计文档和明确的 Bug 复现场景，避免主观臆断和情绪化争论。
2. **所有技术债务必留 Ticket**：如果为了短期目标写了 Hack 代码或 `TODO`，必须在系统里创建对应的重构/清理任务，并在代码中注明注释 `// TODO(#ISSUE_ID): reason`。
3. **系统的透明性 (Observability & Fail-fast)**:
   - **无日志不代码**：关键状态转移（如 Intent 匹配、Action 派发）必须包含 `tracing` 事件。
   - **ERROR vs WARN**：只有当系统无法继续运行或严重违背预期时才使用 `ERROR`；如果是预期内的分支（如网络抖动导致重试），请使用 `WARN` 或 `INFO`。
   - **Lint 增强**：推荐在各模块根部（`lib.rs`/`main.rs`）加入 `#![deny(unsafe_op_in_unsafe_fn, missing_docs)]`，将风险扼杀在编译阶段。
   - **Unsafe 审计**：严格遵守 Rust 安全准则。若必须使用 `unsafe`，必须附带说明：

     ```rust     // SAFETY: 此处已验证 raw_ptr 非空且对齐，生命周期由外部 X 模块保证
     unsafe { ... }
     ```

4. **建设性审查 (Constructive Review)**：在 Code Review 中，审阅者的评论应具体且提供优化方向（例如：“这里可以考虑改用 `BTreeMap`，因为我们需要保持有序遍历”），提交者应对系统质量负责并积极响应改进建议。

5. **错误处理哲学 (Error Handling)**:
   - **库模块 (`lib`)**：必须使用 `thiserror` 定义强类型错误枚举。错误信息须包含上下文（如：哪个 Action 失败）。
   - **应用层 (`bin`)**：可以使用 `anyhow` 处理顶级错误，但严禁在 `Result` 链条中“吞掉”原始错误。
   - **严禁滥用 Panic**：非单元测试代码严禁使用 `unwrap()` 或 `expect()`。对于物理上不可达的分支，必须使用 `unreachable!()` 并附带解释性注释。

---

## 9. 测试哲学：从单元测试到确定性回放

我们的测试不只是为了覆盖率，而是为了**证明状态机在异常面前的鲁棒性**：

1. **确定性回放 (Deterministic Replay)**：利用 `data/traces` 中的离线数据，确保在相同输入下，系统的 Action 序列输出完全一致（State Machine Replication 思想）。
2. **Mock 外部世界**：在 `aios-adapter` 的测试中，必须模拟网络断开、权限被拒、磁盘已满等“地狱模式”场景。
3. **模糊测试 (Fuzzing)**：对于 `aios-spec` 中的序列化/反序列化逻辑，建议引入 `cargo-fuzz` 进行鲁棒性扫描。
4. **数据资产管理 (Data Governance)**：
   - **体积限制**：超过 1MB 的轨迹文件（Traces）严禁直接提交至 Git。必须使用 Git LFS 托管或压缩为 `.bin` 格式。
   - **隐私脱敏**：所有新增的测试轨迹数据必须经过脱敏，严禁包含个人隐私或敏感系统信息。

---

## 10. 开发者工具箱

请熟练使用以下脚本以提高开发效率：

- `scripts/setup-env.sh`: 环境初始化。
- `scripts/check-all.sh`: 提交前的全量质量检查。
- `scripts/android-runner.sh`: 自动将二进制推送到真机执行。
