#!/bin/bash
# scripts/dev/setup-env.sh — DiPECS 环境自检与配置
#
# 分层检查 L0（系统依赖）、L1（Rust 工具链）、L2（Android NDK），
# 并注入 Git hooks 和输出环境概览。
# 必须 source 执行：source scripts/dev/setup-env.sh

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}[DiPECS] 环境自检开始...${NC}"
echo ""

# ── L0: 系统构建依赖 ──────────────────────────────────────────

echo -e "${GREEN}[L0] 系统构建依赖${NC}"

SYSTEM_DEPS=("gcc" "make" "pkg-config" "ld.lld")
MISSING_DEPS=()

check_sys_dep() { command -v "$1" &>/dev/null; }

for dep in "${SYSTEM_DEPS[@]}"; do
    if check_sys_dep "$dep"; then
        echo "  ${GREEN}✓${NC} $dep"
    else
        echo "  ${RED}✗${NC} $dep"
        MISSING_DEPS+=("$dep")
    fi
done

# 动态库检查（pkg-config 存在时才能做）
if command -v pkg-config &>/dev/null; then
    if pkg-config --exists openssl 2>/dev/null; then
        echo "  ${GREEN}✓${NC} libssl-dev (openssl)"
    else
        echo "  ${RED}✗${NC} libssl-dev (openssl)"
        MISSING_DEPS+=("libssl-dev")
    fi
else
    echo "  ${RED}✗${NC} libssl-dev (openssl)"
    MISSING_DEPS+=("libssl-dev")
fi

if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
    echo ""
    echo -e "  ${YELLOW}缺系统包，安装命令：${NC}"
    MISSING_PKGS=()
    for dep in "${MISSING_DEPS[@]}"; do
        case "$dep" in
            gcc|make)      MISSING_PKGS+=("build-essential") ;;
            pkg-config)    MISSING_PKGS+=("pkg-config") ;;
            ld.lld)        MISSING_PKGS+=("lld") ;;
            libssl-dev)    MISSING_PKGS+=("libssl-dev") ;;
        esac
    done
    readarray -t UNIQ_PKGS < <(printf '%s\n' "${MISSING_PKGS[@]}" | sort -u)
    echo "  sudo apt install ${UNIQ_PKGS[*]}"
fi

echo ""

# ── L1: Rust 工具链 ───────────────────────────────────────────

echo -e "${GREEN}[L1] Rust 工具链${NC}"

EXPECTED_CHANNEL="1.95.0"

if ! command -v rustup &>/dev/null; then
    echo -e "  ${RED}✗${NC} rustup 未安装"
    echo -e "  ${YELLOW}安装命令：${NC}"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
    echo "  source \"\$HOME/.cargo/env\""
    echo -e "  ${YELLOW}然后重新 source 本脚本。${NC}"
    echo ""
else
    ACTIVE_CHANNEL=$(rustup show active-toolchain 2>/dev/null | awk '{print $1}' | sed 's/-.*//')
    if [ "$ACTIVE_CHANNEL" = "$EXPECTED_CHANNEL" ]; then
        echo "  ${GREEN}✓${NC} rustup ($(rustup --version 2>/dev/null | awk '{print $2}'))"
        echo "  ${GREEN}✓${NC} toolchain $ACTIVE_CHANNEL"
    else
        echo "  ${GREEN}✓${NC} rustup ($(rustup --version 2>/dev/null | awk '{print $2}'))"
        echo "  ${YELLOW}⚠${NC} toolchain $ACTIVE_CHANNEL（期望 $EXPECTED_CHANNEL）"
        echo -e "  ${YELLOW}  在仓库根目录执行 cargo build 即可自动安装缺失 toolchain${NC}"
    fi

    # 检查 rust-src（rust-analyzer 需要）
    if rustup component list --installed 2>/dev/null | grep -q "rust-src"; then
        echo "  ${GREEN}✓${NC} rust-src"
    else
        echo "  ${YELLOW}⚠${NC} rust-src 未安装（rust-analyzer 需要）"
    fi
fi
echo ""

# ── L2: Android NDK ────────────────────────────────────────────

echo -e "${GREEN}[L2] Android NDK${NC}"

export ANDROID_API=33
export NDK_EXPECTED_VERSION="r27d"

if [ -z "$ANDROID_NDK_HOME" ]; then
    DEFAULT_NDK_PATH="$HOME/Android/ndk/android-ndk-$NDK_EXPECTED_VERSION"
    if [ -d "$DEFAULT_NDK_PATH" ]; then
        export ANDROID_NDK_HOME="$DEFAULT_NDK_PATH"
        echo "  自动检测到 NDK: $ANDROID_NDK_HOME"
    else
        echo -e "  ${YELLOW}⚠${NC} NDK $NDK_EXPECTED_VERSION 未找到"
        echo -e "  ${YELLOW}  不影响本地构建；仅交叉编译 Android 时需要。${NC}"
        echo -e "  ${YELLOW}  下载：https://developer.android.com/ndk/downloads${NC}"
        echo -e "  ${YELLOW}  解压到：$DEFAULT_NDK_PATH${NC}"
        echo -e "  ${YELLOW}  然后 export ANDROID_NDK_HOME=\"$DEFAULT_NDK_PATH\"${NC}"
        NDK_READY=false
    fi
else
    echo "  ${GREEN}✓${NC} ANDROID_NDK_HOME=$ANDROID_NDK_HOME"
    NDK_READY=true
fi

if [ "${NDK_READY:-true}" = true ] && [ -n "${ANDROID_NDK_HOME:-}" ]; then
    OS_TYPE=$(uname -s | tr '[:upper:]' '[:lower:]')
    TOOLCHAIN=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/${OS_TYPE}-x86_64

    if [[ ":$PATH:" != *":$TOOLCHAIN/bin:"* ]]; then
        export PATH="$TOOLCHAIN/bin:$PATH"
    fi

    LINKER_NAME="aarch64-linux-android${ANDROID_API}-clang"
    if command -v "$LINKER_NAME" &>/dev/null; then
        echo "  ${GREEN}✓${NC} linker $LINKER_NAME"
        export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LINKER_NAME"
    else
        echo -e "  ${RED}✗${NC} linker $LINKER_NAME 未在 PATH 中找到"
    fi

    # Rust target（rust-toolchain.toml 理论上已覆盖，此处做兜底检查）
    TARGET_ARCH="aarch64-linux-android"
    if command -v rustup &>/dev/null; then
        if ! rustup target list --installed 2>/dev/null | grep -q "$TARGET_ARCH"; then
            echo -e "  ${YELLOW}正在安装 Rust target: $TARGET_ARCH...${NC}"
            rustup target add "$TARGET_ARCH"
        fi
    fi
fi
echo ""

# ── Git Hooks 注入 ─────────────────────────────────────────────

echo -e "${GREEN}[Hook] Git Hooks${NC}"
GIT_DIR=$(git rev-parse --git-dir 2>/dev/null || true)
if [ -z "$GIT_DIR" ]; then
    echo -e "  ${YELLOW}跳过：当前不在 Git 仓库中${NC}"
else
    mkdir -p "$GIT_DIR/hooks"
    ln -sf ../../scripts/dev/pre-commit.sh "$GIT_DIR/hooks/pre-commit"
    ln -sf ../../scripts/dev/commit-msg.sh "$GIT_DIR/hooks/commit-msg"
    echo "  ${GREEN}✓${NC} pre-commit + commit-msg 已注入"
fi
chmod +x scripts/dev/pre-commit.sh scripts/dev/commit-msg.sh
echo ""

# ── 环境概览 ───────────────────────────────────────────────────

echo -e "${GREEN}[Summary] 环境概览${NC}"
echo "------------------------------------------"
if command -v rustc &>/dev/null; then
    printf "Rustc:       $(rustc --version | awk '{print $2}')\n"
else
    printf "Rustc:       ${RED}NOT FOUND${NC}\n"
fi
if command -v cargo &>/dev/null; then
    printf "Cargo:       $(cargo --version | awk '{print $2}')\n"
else
    printf "Cargo:       ${RED}NOT FOUND${NC}\n"
fi
if command -v adb &>/dev/null; then
    printf "ADB:         $(adb version 2>/dev/null | head -n1 | awk '{print $5}')\n"
else
    printf "ADB:         ${YELLOW}—${NC}\n"
fi
if [ -n "${CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER:-}" ]; then
    printf "Linker:      $CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER\n"
else
    printf "Linker:      ${YELLOW}— (Android 交叉编译不可用)${NC}\n"
fi
echo "------------------------------------------"

# ── 安全检查 ───────────────────────────────────────────────────

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    echo ""
    echo -e "${RED}请使用 source 运行此脚本：source scripts/dev/setup-env.sh${NC}"
    exit 1
fi
