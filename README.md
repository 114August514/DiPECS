# DiPECS

[![Rust](https://img.shields.io/badge/Rust-1.95.0-orange)](rust-toolchain.toml)
[![Android API](https://img.shields.io/badge/Android%20API-33-green)](scripts/setup-env.sh)
[![NDK](https://img.shields.io/badge/NDK-r27d-green)](scripts/setup-env.sh)
[![License](https://img.shields.io/badge/License-Apache--2.0-blue)](LICENSE)

DiPECS (Digital Intelligence Platform for Efficient Computing Systems) 是一个面向 Android/Linux 的 AIOS 原型。它把设备侧观测、隐私脱敏、上下文聚合、决策路由、策略审查和动作执行拆成明确边界，让本地系统保留安全和实时性，并在需要时把脱敏后的结构化上下文交给更强的推理后端。

当前代码以 v0.2 最小闭环为基线，并通过 [RFC-0001](docs/src/design/rfc/0001-layered-collection-and-decision-routing.md) 收紧采集、脱敏、决策和动作边界。更完整的架构说明见 [架构概览](docs/src/design/overview.md) 和 [代码地图](docs/src/design/crates-map.md)。

## Status

已落地：

- `aios-spec` 定义 `RawEvent`、`CollectorEnvelope`、`SanitizedEvent`、`StructuredContext`、`IntentBatch`、`CapabilityLevel` 和 `AuthorizedAction`。
- `apps/android-collector` 验证 Android 用户态采集能力，并导出 JSONL trace 样本。
- `aios-collector` 作为 Rust 采集层入口，统一产出 `CollectorEnvelope` / `RawEvent`。
- `aios-core` 完成隐私脱敏、窗口聚合和策略审查。
- `aios-agent` 提供 `DecisionRouter`、`RuleBasedBackend` 和 `FallbackNoOpBackend`。
- `aios-daemon` 提供 `dipecsd` 长驻运行时。

仍在推进：

- app 到 `aios-collector` 的生产接入通道。
- 本地小模型和云端 LLM 后端。
- 真机动作执行和完整 Golden Trace 回归。

## Architecture

```mermaid
flowchart TD
    App["apps/android-collector<br/>Android API capability"]
    System["daemon / system sources<br/>(later phases)"]
    Collector["aios-collector<br/>ingress + normalize"]
    Raw["CollectorEnvelope / RawEvent"]
    Core["aios-core<br/>PrivacyAirGap + WindowAggregator"]
    Context["StructuredContext"]
    Agent["aios-agent<br/>DecisionRouter"]
    Intent["IntentBatch"]
    Policy["PolicyEngine + CapabilityLevel"]
    Action["aios-action<br/>AuthorizedAction only"]
    Trace["ActionResult / Trace"]

    App -- "JSONL / JNI / socket" --> Collector
    System -- "/proc / Binder / status" --> Collector
    Collector --> Raw --> Core --> Context --> Agent --> Intent
    Intent --> Policy --> Action --> Trace
```

核心边界：

- apps 提供采集能力，`aios-collector` 负责接入与 `RawEvent` 规范化。
- `RawEvent` 不越过 `PrivacyAirGap`；推理层只接收 `StructuredContext`。
- 推理后端只能输出 `IntentBatch`；动作层只执行 `AuthorizedAction`。

## Quick Start

运行 Rust 测试：

```bash
cargo test --workspace
```

以前台模式运行 daemon：

```bash
RUST_LOG=info cargo run -p aios-daemon --bin dipecsd -- --no-daemon
```

配置 Android 交叉编译：

```bash
source scripts/setup-env.sh
cargo android-release
```

构建 Android collector：

```bash
cd apps/android-collector
./gradlew :app:assembleDebug
```

完整开发命令见 [开发指南](docs/src/team/dev.md)，Android collector 细节见 [apps/android-collector/README.md](apps/android-collector/README.md)。

## Repository Map

| Path | Purpose |
| :--- | :--- |
| `crates/aios-spec` | 跨层协议和 trait。 |
| `crates/aios-collector` | Rust 采集层入口。 |
| `crates/aios-core` | 脱敏、聚合、策略审查。 |
| `crates/aios-agent` | 决策路由和模型后端。 |
| `crates/aios-action` | 授权动作执行。 |
| `crates/aios-daemon` | `dipecsd` 运行时装配。 |
| `apps/android-collector` | Android 采集能力验证工具。 |
| `docs/src` | mdBook 工程文档。 |
| `docs/academic` | 学术报告材料。 |

## Documentation

完整工程文档使用 mdBook 管理，CI 自动部署至 [GitHub Pages](https://114august514.github.io/DiPECS/)，工程文档入口见 [站点 Docs 区](https://114august514.github.io/DiPECS/guide/)。

本地预览：

```bash
mdbook build docs        # 构建
mdbook serve docs        # 本地预览
```

- [架构概览](docs/src/design/overview.md)
- [设计哲学](docs/src/design/philosophy.md)
- [Daemon 架构](docs/src/design/daemon-architecture.md)
- [Android 接口 MVP](docs/src/design/android-interface-mvp.md)
- [RFC 流程](docs/src/design/rfc/process.md)
- [RFC-0001 分层采集与决策路由](docs/src/design/rfc/0001-layered-collection-and-decision-routing.md)
- [开发指南](docs/src/team/dev.md)
- [贡献指南](CONTRIBUTING.md)

## License

DiPECS 使用 [Apache License 2.0](LICENSE) 授权。
