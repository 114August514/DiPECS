# 开发指南

## 环境准备

- Rust 1.86.0+
- Android Studio + NDK
- mdBook（文档构建）

## 项目结构

```text
DiPECS/
├── crates/          # Rust 核心模块
├── apps/            # Android 应用
├── docs/            # 文档（mdBook）
├── data/traces/     # Golden Traces
└── scripts/         # 自动化脚本
```

## 构建

```bash
# 本地开发
cargo build --workspace

# Android 交叉编译
cargo build --target aarch64-linux-android

# 文档
mdbook serve docs
```

## CI 检查

提交前确保通过：

- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
