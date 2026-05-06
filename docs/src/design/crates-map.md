# 代码地图

DiPECS 采用严格的”机制与策略分离”架构（详见[架构概览](overview.md)和[设计哲学](philosophy.md)）。本文档是代码仓库的导览地图，说明每个目录和文件的用途。

## 核心工作区 (Crates)

单向依赖，上层依赖下层：

```text
aios-cli                 CLI 工具入口
aios-agent               daemon 二进制 (dipecsd) + CloudProxy
    ↓
aios-adapter             Binder、/proc、文件系统采集 (纯库)
    ↓
aios-kernel              资源管理 + ActionExecutor
    ↓
aios-core                ActionBus、PolicyEngine、PrivacyAirGap、WindowAggregator
    ↓
aios-spec                数据类型 + Trait 定义 (零外部依赖)
```

- **`crates/aios-spec/`** `src/lib.rs` — 核心数据结构、Trait 接口和 Protobuf IDL。禁止业务逻辑或平台依赖
- **`crates/aios-core/`** `src/lib.rs` + `src/context_builder.rs` — 状态机、Action 调度、脱敏引擎、策略引擎、窗口聚合。纯同步逻辑
- **`crates/aios-kernel/`** `src/lib.rs` — 资源管理与 DefaultActionExecutor 骨架
- **`crates/aios-adapter/`** `src/lib.rs` — Binder 调用、/proc 读取、OfflineAdapter。纯库，不含二进制入口
- **`crates/aios-agent/`** `src/lib.rs` + `src/main.rs` — MockCloudProxy + **daemon 二进制入口** (`dipecsd`)，含采集循环和完整处理管道
- **`crates/aios-cli/`** `src/main.rs` — 命令行交互工具

## 文档生态 (Docs)

双轨知识库体系，供工程协作与学术验收。

- **`docs/src/`**: 工程知识库 (mdBook)
  - `design/states.md`: 系统状态机核心设计文档。
  - `team/dev.md`: 开发者指南。
  - `design/rfc/`: 架构设计提案 (Request for Comments) 存放处。
- **`docs/academic/`**: 学术验收库 (LaTeX)
  - `01_Survey_Report/` 至 `04_Final_Report/`: 课题调研、可行性、中期及结题报告的 LaTeX 源码。

## 脚本与工具 (Scripts)

- **`scripts/android-runner.sh`**: 用于在 Android (NDK/API 33) 上的交叉编译部署与运行脚本。
- **`scripts/check-all.sh`**: 核心 CI 脚本，执行格式化、Clippy 验证、构建与测试。
- **`scripts/setup-env.sh`**: 初始化 Rust 工具链和预编译环境的脚本。

## 数据与测试 (Data & Tests)

- **`data/traces/`**: 收集的离线系统轨迹数据，用于 `OfflineAdapter` 的系统状态机物理回滚与验证。
- **`data/evaluation/`**: 系统性能与准确性评估数据集。
- **`tests/integration/`**: 跨 Crates 的集成测试用例，确保状态转移的正确性。

## 根目录配置文件

- **`Cargo.toml`**: Cargo Workspace 配置文件，统筹所有 Crates。
- **`rust-toolchain.toml`**: 锁定 Rust 版本（要求 1.86.0）及交叉编译 Target。
- **`deny.toml`**: 依赖项审计配置，防止引入不安全的重型大体积 Crate。
- **`rustfmt.toml`**: 全局代码格式化规范。
- **`README.md` & `CONTRIBUTING.md`**: 项目门面介绍及贡献指南。
