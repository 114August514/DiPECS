#!/bin/bash
# scripts/check-all.sh

set -e # 遇错即止

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}=== 开始全量自动化检查 ===${NC}"

# 0. 环境前置检查
if [ -z "$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER" ]; then
    echo -e "${YELLOW}[提示] 正在自动加载环境配置...${NC}"
    source "$(dirname "$0")/setup-env.sh" || exit 1
fi

echo -e "\n${GREEN}--- 1. 格式化检查 (Rustfmt) ---${NC}"
cargo fmt --all -- --check

echo -e "\n${GREEN}--- 2. 静态分析 (Clippy) ---${NC}"
# 建议同时针对安卓目标进行 clippy，因为有些代码可能使用了 #[cfg(target_os = "android")]
cargo clippy --workspace --all-targets -- -D warnings

echo -e "\n${GREEN}--- 3. 单元测试 (本地) ---${NC}"
cargo test --workspace

echo -e "\n${GREEN}--- 4. 交叉编译检查 (Android) ---${NC}"
# 增加 --all-targets 确保测试代码和示例代码在安卓下也能过
cargo check --workspace --target aarch64-linux-android --all-targets

echo -e "\n${GREEN} 所有检查通过！代码质量达标，可以提交。${NC}"
