#!/bin/bash
# scripts/setup-env.sh

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}[DiPECS] 配置 Android 交叉编译环境...${NC}"

# --- [ 策略硬编码：全队统一标准 ] ---
export ANDROID_API=33
# export ANDROID_API=${ANDROID_API:-33}  # 如果用户没设置，默认使用 33
export NDK_EXPECTED_VERSION="r27d"

# 1. 检查环境变量
if [ -z "$ANDROID_NDK_HOME" ]; then
    # 尝试推测路径
    DEFAULT_NDK_PATH="$HOME/Android/ndk/android-ndk-$NDK_EXPECTED_VERSION"
    if [ -d "$DEFAULT_NDK_PATH" ]; then
        export ANDROID_NDK_HOME="$DEFAULT_NDK_PATH"
        echo "自动检测到 NDK: $ANDROID_NDK_HOME"
    else
        echo -e "${RED}错误: 未设置 \$ANDROID_NDK_HOME 且在默认路径未找到 NDK $NDK_EXPECTED_VERSION${NC}"
        echo "请先下载 NDK $NDK_EXPECTED_VERSION 并设置环境变量。"
        echo "例如: export ANDROID_NDK_HOME=$HOME/path/to/android-ndk"
        return 1 2>/dev/null || exit 1
    fi
fi

# 2. 动态定位 LLVM 工具链 (适配 Linux-x86_64)
OS_TYPE=$(uname -s | tr '[:upper:]' '[:lower:]')
export TOOLCHAIN=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/${OS_TYPE}-x86_64

# 3. 将工具链 bin 目录加入当前会话的 PATH
# 这样 .cargo/config.toml 里的 "aarch64-linux-android33-clang" 才能被找到
if [[ ":$PATH:" != *":$TOOLCHAIN/bin:"* ]]; then
    export PATH="$TOOLCHAIN/bin:$PATH"
    echo "已将 NDK Toolchain 加入 PATH"
fi

# 3.5 增加：Rust Target 自动检查与补充
TARGET_ARCH="aarch64-linux-android"
if ! rustup target list --installed | grep -q "$TARGET_ARCH"; then
    echo -e "${YELLOW}[DiPECS] 正在安装 Rust target: $TARGET_ARCH...${NC}"
    rustup target add "$TARGET_ARCH"
fi

# 4. 定义 API 版本和链接器名称
LINKER_NAME="aarch64-linux-android${ANDROID_API}-clang"

# 5. 验证链接器是否存在
if command -v "$LINKER_NAME" &> /dev/null; then
    echo -e "${GREEN}Android API ${ANDROID_API} 交叉编译工具链就绪！${NC}"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LINKER_NAME"
else
    echo -e "${RED}错误: 未找到 $LINKER_NAME${NC}"
    echo "请检查 $TOOLCHAIN/bin 目录下是否存在该文件。"
    return 1 2>/dev/null || exit 1
fi

# 6. [自举 Bootstrap] 注入 GitHooks
echo "正在注入 DiPECS Git Hooks..."
GIT_DIR=$(git rev-parse --git-dir 2>/dev/null || true)
if [ -z "$GIT_DIR" ]; then
    echo -e "${RED}错误: 当前不在 Git 仓库中，跳过 Git Hooks 注入。${NC}"
else
    mkdir -p "$GIT_DIR/hooks"
    # 注入 pre-commit (基于相对路径，增强可移植性)
    ln -sf ../../scripts/pre-commit.sh "$GIT_DIR/hooks/pre-commit"
    # 注入 commit-msg
    ln -sf ../../scripts/commit-msg.sh "$GIT_DIR/hooks/commit-msg"
    echo -e "${GREEN}已启动 物理检查站 & 语义检查站。${NC}"
fi

chmod +x scripts/pre-commit.sh scripts/commit-msg.sh

# 6.5 环境自检报告
echo -e "\n${GREEN}[Check] 环境概览:${NC}"
echo "------------------------------------------"
printf "Rustc:   $(rustc --version | awk '{print $2}')\n"
printf "Cargo:   $(cargo --version | awk '{print $2}')\n"
if command -v adb &> /dev/null; then
    printf "ADB:     $(adb version | head -n1 | awk '{print $5}')\n"
else
    printf "ADB:     ${RED}NOT FOUND${NC}\n"
fi
printf "Linker:  $CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER\n"
echo "------------------------------------------"

# 7. 安全检查：提示用户使用 source 运行
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    echo -e "${RED}请使用 source 运行此脚本，例如: source scripts/setup-env.sh${NC}"
    exit 1
fi