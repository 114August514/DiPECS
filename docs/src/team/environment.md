# 环境配置

从一台刚装好 OS 的 x86-64 机器出发，到达 `cargo build --workspace` 和 `cargo test --workspace` 全部通过。

## 前置条件

- Linux x86-64（Ubuntu 24.04 / Debian 12+），含 **WSL2**
- `sudo` 权限
- 网络连接
- `git`（`sudo apt install git`）

> **WSL2 注意：** 仓库必须放在 WSL 原生文件系统下（`~/`），不要放在 `/mnt/c/` 下——跨文件系统 I/O 会让 `cargo build` 慢一个数量级。从 Windows 侧 clone 的仓库请 `cp -r` 进 WSL 再操作。

## 必装层

### L0 — 系统构建依赖

Rust 编译链在 OS 层的硬前提：

```bash
sudo apt install build-essential pkg-config libssl-dev lld
```

- `build-essential`：gcc、make 等基础构建工具
- `pkg-config` + `libssl-dev`：TLS / crypto 库（tokio、reqwest 等 crate 的传递依赖）
- `lld`：LLVM 链接器（`.cargo/config.toml` 中对 `x86_64-unknown-linux-gnu` 和 Android target 都指定了 `-fuse-ld=lld`）

### L1 — Rust 工具链

通过 rustup 安装（**不要**用 apt 或 snap 的 rustup 包，版本滞后且不受项目锁定）：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

之后在仓库根目录执行任意 cargo 命令（如 `cargo build`），rustup 会自动读取 `rust-toolchain.toml` 并安装缺失的 toolchain、组件和目标平台：

```bash
# 仅查看当前 toolchain 状态（不会触发安装）
rustup show

# 首次构建会触发自动安装
cargo build --workspace
```

这会自动安装：

- channel：`1.95.0`（`rust-toolchain.toml` 锁定）
- components：`rustfmt`、`clippy`、`rust-src`、`rust-analyzer`、`llvm-tools-preview`
- targets：`x86_64-unknown-linux-gnu`、`aarch64-linux-android`、`x86_64-linux-android`

> 无需单独 `rustup target add`，`rust-toolchain.toml` 中的 `targets` 字段已覆盖。

### L2 — Android NDK

NDK 仅交叉编译 Android target 时需要；不碰 Android 的成员可以跳过，`cargo build --workspace`（不含 `--target aarch64-linux-android`）不受影响。

1. 下载 NDK r27d：

   ```bash
   # 手动下载（推荐）
   # 地址：https://developer.android.com/ndk/downloads
   # 选择 "NDK r27d Linux (x86-64)"

   # 或命令行下载
   wget https://dl.google.com/android/repository/android-ndk-r27d-linux.zip
   unzip android-ndk-r27d-linux.zip -d ~/Android/ndk/
   ```

2. 设置环境变量（写入 `~/.bashrc` 或等效文件）：

   ```bash
   export ANDROID_NDK_HOME="$HOME/Android/ndk/android-ndk-r27d"
   export PATH="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"
   ```

3. 验证：

   ```bash
   aarch64-linux-android33-clang --version
   ```

   注意：这是 Rust daemon 交叉编译当前使用的链接器 API 级别；Android app 自身的
   `compileSdk`/`targetSdk` 为 35，`minSdk` 为 26。

以上 L0–L2 也可以一键执行：

```bash
source scripts/setup-env.sh
```

脚本会检查各层状态并报告缺失项。

## 按需层（L3）

以下工具不影响编译核心链路，按角色按需安装。

| 工具 | 用途 | 谁需要 | 安装方式 |
| --- | --- | --- | --- |
| `adb` | Android 设备部署 / 日志 | 移动端开发 | `sudo apt install adb`；WSL2 需 [usbipd](https://github.com/dorssel/usbipd-win) 或走 `adb connect` 网络调试 |
| `uv` + `MkDocs` | 文档构建与预览 | 写文档的人 | `cd docs && uv sync` |
| `cargo-deny` | 依赖审计（CI 中也会跑） | 改 `Cargo.toml` 的人 | `cargo install cargo-deny` |

## 验证

```bash
# 核心链路
cargo build --workspace
cargo test --workspace

# 全量 CI 检查
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Android 交叉编译（需要 L2）
source scripts/setup-env.sh
cargo build --target aarch64-linux-android --release
```

## 环境配置的边界

| 归它管 | 不归它管 |
| --- | --- |
| 系统构建依赖 | IDE / 编辑器 |
| Rust 工具链 | Shell 美化 |
| Android NDK + linker | Git 身份（name/email/key） |
| Git hooks（`setup-env.sh` 自动注入） | SSH / GitHub 认证 |
| 可选工具（adb、MkDocs） | Docker / 容器 |
| | CI runner 配置 |

原则：**只配"不配就编不过"的东西。**
