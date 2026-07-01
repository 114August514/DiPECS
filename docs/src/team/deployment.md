# 部署指南

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `.cargo/config.toml`, `scripts/setup-env.sh`, `scripts/android-runner.sh`, `apps/android-collector/scripts/ship-system.sh`

**这篇文档回答什么**：如何把 DiPECS 从源码构建、部署到 Android 模拟器/真机，以及如何打包 release。  
**适合谁读**：需要在设备上运行 daemon 或发布 APK 的人。

## TL;DR

1. 装依赖：`build-essential pkg-config libssl-dev lld`。
2. 装 Rust toolchain（仓库 `rust-toolchain.toml` 自动锁定）。
3. 装 NDK r27d 并导出 `ANDROID_NDK_HOME`。
4. `cargo build --target aarch64-linux-android --release`。
5. 用 `adb push` 把 `dipecsd` 推到设备并运行；或 `./scripts/run-daemon-in-emulator.sh`。

## 环境准备

### 系统依赖

```bash
sudo apt install build-essential pkg-config libssl-dev lld
```

### Rust toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

在仓库根目录执行任意 cargo 命令会自动安装 `rust-toolchain.toml` 中指定的 channel、components 和 targets：

- channel：`1.95.0`
- targets：`x86_64-unknown-linux-gnu`、`aarch64-linux-android`、`x86_64-linux-android`

### NDK

```bash
wget https://dl.google.com/android/repository/android-ndk-r27d-linux.zip
unzip android-ndk-r27d-linux.zip -d ~/Android/ndk/
export ANDROID_NDK_HOME="$HOME/Android/ndk/android-ndk-r27d"
export PATH="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"
```

验证：

```bash
aarch64-linux-android33-clang --version
```

注意：Rust daemon 当前使用 API 33 链接器；Android app 的 `compileSdk`/`targetSdk` 是 35。

### 一键自检

```bash
source scripts/setup-env.sh
```

该脚本会检查系统依赖、toolchain、NDK，安装 git hooks，并把 NDK 工具链加入 PATH。

## 构建 Rust daemon

### Linux 本地

```bash
cargo build --workspace --release
```

### Android ARM64

```bash
cargo build --workspace --target aarch64-linux-android --release
```

或只构建 daemon：

```bash
cargo build -p aios-daemon --bin dipecsd --release --target aarch64-linux-android
```

仓库已配置 cargo alias：

```bash
cargo android-release   # 等价于 cargo build --target aarch64-linux-android --release
```

### Android x86_64（模拟器）

```bash
cargo build -p aios-daemon --bin dipecsd --release --target x86_64-linux-android
```

## 运行 dipecsd

### 手动 push + 运行

```bash
adb push target/aarch64-linux-android/release/dipecsd /data/local/tmp/dipecsd
adb shell chmod +x /data/local/tmp/dipecsd
adb shell /data/local/tmp/dipecsd --no-daemon
```

带 Android JSONL 输入：

```bash
adb push data/traces/sample_replay.jsonl /data/local/tmp/sample.jsonl
adb shell /data/local/tmp/dipecsd --no-daemon \
  --android-trace-jsonl /data/local/tmp/sample.jsonl
```

### 使用 cargo runner

`.cargo/config.toml` 配置了 `scripts/android-runner.sh`，运行时会自动 push 并执行：

```bash
ADB_SERIAL=<serial> cargo run -p aios-daemon --bin dipecsd --release --target aarch64-linux-android
```

### 模拟器一键脚本

```bash
./scripts/run-daemon-in-emulator.sh --attach
```

默认使用 x86_64 构建、bridge token `dipecs-dev-emulator-shared-token-00000000`、端口 46321。

### 设备内闭环验证

```bash
bash tests/scenarios/on-device-dipecsd.sh
```

该脚本会检测 ABI、交叉编译、push 二进制和 JSONL、运行 dipecsd、验证 action loop 闭环。

## 构建 Android APK

### Debug

```bash
cd apps/android-collector
./gradlew :app:assembleDebug
```

输出：`app/build/outputs/apk/debug/app-debug.apk`

### Release

```bash
./gradlew :app:assembleRelease
```

输出：`app/build/outputs/apk/release/app-release.apk`

默认使用 `~/.android/debug.keystore`。

### Platform-signed 系统应用

创建 `apps/android-collector/signing/platform.properties`：

```properties
platform.storeFile=/path/to/platform.keystore
platform.storePassword=android
platform.keyAlias=platform
platform.keyPassword=android
platform.certificateFile=/path/to/platform.x509.pem
```

构建：

```bash
./gradlew :app:assemblePlatform -PDIPECS_PLATFORM_SIGNING=true
```

## 系统镜像集成

使用 `apps/android-collector/scripts/ship-system.sh` 把 platform-signed APK 和 daemon 二进制刷入 system 分区：

```bash
cd apps/android-collector
./scripts/ship-system.sh
```

该脚本会：

1. 构建 platform-signed APK
2. 交叉编译 `dipecsd` for ARM64
3. `adb root` + `adb remount`
4. 推送 `dipecsd` 到 `/system/bin/dipecsd`
5. 推送 APK 到 `/system/priv-app/DiPECSCollector/`
6. 设置 `persist.dipecs.bridge.token`
7. 重启设备

其他模式：

```bash
./scripts/ship-system.sh --build-only
./scripts/ship-system.sh --start-only
./scripts/ship-system.sh --stop
./scripts/ship-system.sh --uninstall
./scripts/ship-system.sh --verify
```

验证系统部署：

```bash
adb shell /system/bin/dipecsd --version
adb shell "ss -tlnp | grep 46321"
adb shell pm list packages -f com.dipecs.collector
```

## CI 发布

- `.github/workflows/android-collector.yml`：debug 构建 + 单元测试。
- `.github/workflows/android-collector-release.yml`：在 `v*` tag 上创建 GitHub Release 并上传 `app-release.apk`。
- `.github/workflows/build.yml`：交叉编译 Linux/Android release 二进制并做 ELF audit。

## 相关文档

- [环境配置](../team/environment.md)
- [开发指南](../team/dev.md)
- [Android 动作实现手册](../android/action-bridge.md)
- [Android 真机/模拟器验证](../android/real-device-validation.md)
