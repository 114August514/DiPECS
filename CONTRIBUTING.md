# Contributing to DiPECS

DiPECS 是一个处在快速演进中的 AIOS 原型。本文件只说明参与开发需要遵守的最低规则；背景、架构和学术文档请阅读 [文档中心](https://114august514.github.io/DiPECS/)。

## Before You Start

- Bug 修复：先确认是否已有 Issue；新 Issue 需要包含复现步骤、期望行为和实际行为。
- 新功能或协议变更：先走 [RFC 流程](https://114august514.github.io/DiPECS/rfc/process/)，尤其是会影响 `aios-spec`、跨 crate 数据结构或模块边界的改动。
- 大改动：先在 Issue / RFC 中说明范围，避免一次 PR 混入架构、实现、格式化和文档重写。

## Setup

项目工具链由仓库锁定。详细的环境配置步骤见 [环境配置指南](https://114august514.github.io/DiPECS/team/environment/)。

一键自检：

```bash
source scripts/setup-env.sh
```

验证：

```bash
cargo build --workspace
cargo test --workspace
```

本地开发、Android 部署和日志命令见 [开发指南](https://114august514.github.io/DiPECS/team/dev/)。

## Local Checks

提交 PR 前至少运行：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

需要验证 Android target 时运行：

```bash
./scripts/check-all.sh
```

CI 红了不能合并。依赖审计和 CI 细节见 [CI 质检体系](https://114august514.github.io/DiPECS/team/ci/)。

## Architecture Rules

模块边界是本项目最重要的约束：

- `aios-spec` 是协议单一事实来源，不依赖业务模块。
- `apps/android-collector` 提供 Android 采集能力，不生产最终 `StructuredContext`。
- `aios-collector` 是 Rust 采集层入口，负责接入 app/system 来源并输出 `CollectorEnvelope` / `RawEvent`。
- `aios-core` 负责隐私脱敏、窗口聚合和策略审查。
- `aios-agent` 只接收 `StructuredContext`，统一输出 `IntentBatch`。
- `aios-action` 只执行 `PolicyEngine` 授权后的 `AuthorizedAction`，并依赖 `aios-core` 获取 `ActionAdapter` trait 和 `AuthorizedAction` 类型。
- `aios-daemon` 只做运行时装配和生命周期管理。

依赖方向：`aios-spec -> collector/core/agent -> aios-daemon`，`aios-action` 额外依赖 `aios-core`（为了 `ActionAdapter` trait 和 `AuthorizedAction` 类型）。禁止循环依赖，禁止让 action 读取采集或推理内部状态。详细说明见 [代码地图](https://114august514.github.io/DiPECS/architecture/crates/) 和 [RFC-0001](https://114august514.github.io/DiPECS/rfc/0001-layered-collection-and-decision-routing/)。

## Pull Requests

使用 feature branch 工作流：

```text
feat/<short-name>
fix/<short-name>
docs/<short-name>
```

PR 要求：

- 描述问题、改动和验证命令。
- 协议或架构变化链接对应 RFC / Issue。
- 保持改动聚焦；无关格式化和重构另开 PR。
- 至少一名相关模块维护者 review；涉及跨模块协议时需要更严格审查。

Commit 建议使用 Conventional Commits：

```text
feat(spec): add collector envelope
fix(core): reject over-capability actions
docs(readme): clarify collector boundary
```

## Testing Expectations

- 新 `RawEvent`：补 `aios-spec` 类型、`PrivacyAirGap` 脱敏测试、窗口聚合测试。
- 新决策规则：补 `aios-agent` 后端测试，并说明后端能力上限。
- 新动作：补 `PolicyEngine` 审查测试和 `aios-action` 执行结果测试。
- Android 采集能力：先在 `apps/android-collector` 证明真实 API 能看到稳定字段，再接入 `aios-collector`。
- 端到端回路变更：补充或更新 `tests/scenarios/` 下的场景脚本。action-loop 变更需通过 mock-socket 验证。

## Safety

- 非测试代码避免 `unwrap()` / `expect()`；库层使用结构化错误。
- 原始文本、完整路径、联系人、通知正文等敏感信息不得越过 `PrivacyAirGap`。
- 任何自动动作必须经过 `PolicyEngine` 和 `CapabilityLevel` 审查。
- 新依赖需要说明必要性、Android 交叉编译支持和二进制体积影响。

## Useful Links

- [README](README.md)
- [开发指南](https://114august514.github.io/DiPECS/team/dev/)
- [架构概览](https://114august514.github.io/DiPECS/architecture/)
- [RFC 流程](https://114august514.github.io/DiPECS/rfc/process/)
- [团队分工](https://114august514.github.io/DiPECS/team/roles/)
