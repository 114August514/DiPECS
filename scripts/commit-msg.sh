#!/bin/bash
# scripts/commit-msg.sh

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

COMMIT_MSG_FILE=$1

# 0. 防御性检查：确保输入文件真实物理存在
if [ ! -f "$COMMIT_MSG_FILE" ]; then
    echo -e "${RED}错误: 找不到 Commit Message 临时文件: $COMMIT_MSG_FILE${NC}"
    exit 1
fi

COMMIT_MSG=$(cat "$COMMIT_MSG_FILE")

echo -e "${GREEN}[AIOS-Gatekeeper] 正在校验 Commit Message 规范...${NC}"

# 1. 忽略合并 (Merge) 或 恢复 (Revert) 自动生成的消息
if echo "$COMMIT_MSG" | grep -qE "^(Merge|Revert)"; then
    exit 0
fi

# 1.5 长度检查 (拒绝毫无意义的提交)
if [ ${#COMMIT_MSG} -lt 10 ]; then
    echo -e "${RED}错误: Commit message 太短 (${#COMMIT_MSG} 字符)，请描述具体做了什么！${NC}"
    echo "提示: 优秀的开发者不应该提交 'fix' 或 'update' 这种含糊的消息。"
    exit 1
fi

# 2. 核心正则表达式检查 (Conventional Commits)
# 格式: <type>(<scope>): <subject>
# 类型: feat|fix|docs|style|refactor|perf|test|chore|ci|build
if ! echo "$COMMIT_MSG" | grep -qE '^(feat|fix|docs|style|refactor|perf|test|chore|ci|build)(\([^)]+\))?!?: .+$'; then
    echo -e "${RED}错误: 不符合 Conventional Commits 规范！${NC}"
    echo -e "当前消息: ${YELLOW}$COMMIT_MSG${NC}"
    echo ""
    echo "💡 建议格式: <type>(<scope>): <subject>"
    echo "常见类型: "
    echo "  feat     - 新功能"
    echo "  fix      - 修复 Bug"
    echo "  docs     - 文档更新"
    echo "  refactor - 代码重构"
    echo "  chore    - 琐事/构建工具更新"
    echo ""
    echo "💡 如需强制提交（不符合规范），请使用 git commit --no-verify"
    exit 1
fi

echo -e "${GREEN}Commit Message 格式合法${NC}"
exit 0
