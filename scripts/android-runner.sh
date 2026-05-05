#!/bin/bash
# scripts/android-runner.sh

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

# Cargo 会把编译好的二进制文件作为第一个参数传进来
BINARY_PATH=$1
BINARY_NAME=$(basename "$BINARY_PATH")

# 定义手机上的临时存放路径
DEST_DIR="/data/local/tmp/dipecs"

# 增加：支持多设备时指定设备 (例: ADB_SERIAL=xxxx cargo run)
ADB_CMD="adb ${ADB_SERIAL:+-s $ADB_SERIAL}"

# 1. 检查设备连接状态
if ! $ADB_CMD get-state &> /dev/null; then
    echo -e "${RED}[Runner] 错误: 未检测到设备。请连接手机或设置 ADB_SERIAL。${NC}"
    exit 1
fi

echo -e "${GREEN}[Runner] 正在推送 $BINARY_NAME 到手机...${NC}"

# 2. 推送并运行 (使用 -e 确保命令链在出错时中断)
set -e
$ADB_CMD shell "mkdir -p $DEST_DIR"
# 使用 sync 逻辑可以稍微加快重复推送的速度
$ADB_CMD push "$BINARY_PATH" "$DEST_DIR/"

echo -e "${GREEN}[Runner] 正在启动程序并监听输出...${NC}"
echo "------------------------------------------"

# 3. 核心运行逻辑：
# - chmod +x: 确保可执行
# - LD_LIBRARY_PATH: 包含当前目录，防止动态库找不到
# - 程序运行结束后返回其状态码
$ADB_CMD shell "chmod +x $DEST_DIR/$BINARY_NAME && LD_LIBRARY_PATH=$DEST_DIR $DEST_DIR/$BINARY_NAME"

# 4. 捕获 shell 的退出状态
EXIT_CODE=$?
echo "------------------------------------------"
if [ $EXIT_CODE -ne 0 ]; then
    echo -e "${RED}[Runner] 程序运行异常退出 (Exit Code: $EXIT_CODE)${NC}"
    exit $EXIT_CODE
else
    echo -e "${GREEN}[Runner] 程序运行结束。${NC}"
fi

# 5. (注释掉清理逻辑，方便调试)
# echo "[Runner] 正在清理..."
# adb shell "rm $DEST_DIR/$BINARY_NAME"