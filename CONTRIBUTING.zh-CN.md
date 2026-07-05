# DiPECS 贡献指南

[English](CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md)

DiPECS 是一个快速演进中的 Android AIOS 研究原型。本指南说明参与代码、实验
artifact 和文档维护时需要遵守的最低规则。背景和架构请阅读
[文档站点](https://114august514.github.io/DiPECS/)。

## 开始之前

- Bug 修复：先确认是否已有 issue；新 issue 应包含复现步骤、期望行为和实际行为。
- 新功能或协议变更：先开或更新 RFC，尤其是影响 `aios-spec`、跨 crate 数据结构、
  动作治理或 Android bridge contract 的改动。
- 大改动：实现前先在 issue / RFC 中说明范围，避免一个 PR 混入架构、功能、格式化和
  无关文档重写。

## 环境准备

仓库锁定 Rust 工具链和 Android 侧假设。先运行：

```bash
source scripts/setup-env.sh
```

基础验证：

```bash
cargo build --workspace
cargo test --workspace
```

Android 相关工作需要 Android SDK Platform 35 和 NDK r27d。详细环境说明见项目开发文档。

## 本地检查

开 PR 前至少运行：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

更完整的本地 CI 风格检查：

```bash
bash scripts/dev/check-all.sh
```

文档改动需要运行：

```bash
cd docs
uv run env PYTHONPATH=. mkdocs build
```

## 架构规则

模块边界是设计的一部分，不是实现细节：

- `aios-spec` 是协议单一事实来源，不能依赖业务模块。
- `apps/android-collector` 负责 Android public API 采集和 action bridge，不生产最终
  `StructuredContext`。
- `aios-collector` 接入 app/system 来源并输出 `CollectorEnvelope` / `RawEvent`。
- `aios-core` 负责隐私脱敏、窗口聚合、策略审查、replay 验证和 `AuthorizedAction`
  生命周期 seal。
- `aios-agent` 只接收脱敏后的上下文，并输出 `IntentBatch`。
- `aios-action` 只执行策略授权动作和 Android-safe bridge 子集。
- `aios-daemon` 只做运行时装配和生命周期管理。

禁止循环依赖。不要让动作执行层直接读取采集或推理内部状态。

## Pull Request

使用 feature branch：

```text
feat/<short-name>
fix/<short-name>
docs/<short-name>
```

PR 应包含：

- 问题描述、实现摘要和验证命令。
- 涉及协议、架构或证据策略的改动需链接 issue / RFC。
- 聚焦 diff；无关格式化和重构单独开 PR。
- 至少由相关模块维护者 review；跨模块 contract 需要更严格审查。

建议使用 Conventional Commits：

```text
feat(action): add volatile cache release target
fix(android): reject stale execute envelopes
docs(readme): clarify v0.3 evidence boundary
```

## 测试要求

- 新 `RawEvent`：更新 `aios-spec`，补 `PrivacyAirGap` 测试和窗口聚合覆盖。
- 新决策规则：补 `aios-agent` 后端测试，并说明能力上限。
- 新动作：补 `PolicyEngine` 审查测试、`aios-action` 结果测试；若发往设备，还要补
  Android bridge 覆盖。
- Android 采集能力：先在 `apps/android-collector` 证明字段稳定，再接入
  `aios-collector`。
- 端到端路径变更：补充或更新 `tests/scenarios/` 下的场景脚本。
- 证据主张：明确 n、设备、source artifact、baseline 和 acceptance gate。

## 安全与隐私

- 非测试代码避免 `unwrap()` / `expect()`；使用结构化错误。
- 原始文本、完整路径、联系人、通知正文、token 和设备唯一标识不得越过
  `PrivacyAirGap`，也不得进入提交的 artifact。
- 自动动作必须经过 `PolicyEngine`、`CapabilityLevel` 和 `ActionLifecycle`。
- 新依赖需要说明必要性；涉及 Android 时说明交叉编译情况和二进制体积影响。
- 真机脚本在 bridge 响应、压力证据或 cache artifact 缺失时必须 fail closed。

## 文档与 i18n

- 根目录用户入口文档需要保持英文和简体中文两份：
  `README.md` / `README.zh-CN.md`，`CONTRIBUTING.md` / `CONTRIBUTING.zh-CN.md`。
- 修改一种语言时，同一个 PR 中同步更新另一种语言。
- 技术数字在两种语言中必须一致；不要把证据边界翻译丢。
- 根文档保持简洁，详细说明链接到 `docs/src`。

## 常用链接

- [README](README.md)
- [中文 README](README.zh-CN.md)
- [Changelog](CHANGELOG.md)
- [架构概览](docs/src/architecture/index.md)
- [动作收益覆盖](docs/src/evaluation/action-benefit-coverage.md)
- [测试指南](tests/README.md)
- [第三方来源](third_party/README.md)
