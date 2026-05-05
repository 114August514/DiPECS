#!/bin/bash
# scripts/pre-commit.sh

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# 配置变量
SKIP_CLIPPY=${SKIP_CLIPPY:-0}
TIMEOUT_CLIPPY=${TIMEOUT_CLIPPY:-120}  # 秒
STRICT_TEST=${STRICT_TEST:-0}  # 测试失败是否阻止提交，0 = 警告，1 = 阻止
CHECK_COMMIT_MSG=${CHECK_COMMIT_MSG:-0}  # 是否校验 commit message
CHECK_MARKDOWN=${CHECK_MARKDOWN:-1}  # 是否检查 Markdown 文件 (默认开启)
CHECK_ANDROID_TARGET=${CHECK_ANDROID_TARGET:-0}  # 是否进行 Android 交叉编译预检 (默认关闭)

echo -e "${GREEN}[AIOS-Gatekeeper] 启动提交前状态检查...${NC}"

# 1. 依赖检查
echo "检查依赖可用性..."
for cmd in cargo git; do
    if ! command -v $cmd &> /dev/null; then
        echo -e "${RED}错误: 找不到 '$cmd' 命令。请确保已安装。${NC}"
        exit 1
    fi
done

# 2. 检查代码格式
echo "检查代码格式 (cargo fmt)..."
cargo fmt --all -- --check > /dev/null 2>&1
if [ $? -ne 0 ]; then
    echo -e "${RED}错误: 代码格式不规范！请运行 'cargo fmt' 修复。${NC}"
    exit 1
fi

# 3. 静态分析 (Clippy) - 支持跳过
if [ "$SKIP_CLIPPY" -eq 0 ]; then
    echo "执行静态分析 (cargo clippy)..."
    timeout $TIMEOUT_CLIPPY cargo clippy --workspace -- -D warnings
    CLIPPY_EXIT=$?
    if [ $CLIPPY_EXIT -eq 124 ]; then
        echo -e "${YELLOW}⚠ 警告: Clippy 检查超时 (${TIMEOUT_CLIPPY}s)。${NC}"
        echo "提示: 使用 'SKIP_CLIPPY=1 git commit' 跳过此检查。"
        exit 1
    elif [ $CLIPPY_EXIT -ne 0 ]; then
        echo -e "${RED}错误: Clippy 检查未通过！请修复所有警告。${NC}"
        exit 1
    fi
else
    echo -e "${YELLOW}⏭ 跳过 Clippy 检查 (SKIP_CLIPPY=1)${NC}"
fi

# 4. 检查大文件（修复逻辑）
echo "检查大文件是否错误进入 Git..."
# 使用数组存储，仅检查暂存的文件
declare -a BIG_FILES
while IFS= read -r file; do
    size=$(git cat-file -s ":$file" 2>/dev/null || echo 0)
    if [ "$((size))" -gt 1048576 ]; then
        BIG_FILES+=($(printf '%s|%d' "$file" "$size"))
    fi
done < <(git diff --cached --name-only --diff-filter=ACM)

if [ ${#BIG_FILES[@]} -gt 0 ]; then
    echo -e "${RED}错误: 发现未经过 LFS 管理的大文件:${NC}"
    for entry in "${BIG_FILES[@]}"; do
        IFS='|' read -r file size <<< "$entry"
        size_mb=$(awk -v s="$size" 'BEGIN {printf "%.1f", s/1048576}')
        printf '  %s (%sMB)\n' "$file" "$size_mb"
    done
    if git rev-parse --verify HEAD > /dev/null 2>&1; then
        echo ""
        echo "💡 修复建议:"
        echo "  1. 确保环境就绪: source scripts/setup-env.sh"
        echo "  2. 安装 Git LFS: https://git-lfs.github.com/"
        echo "  3. 配置追踪: git lfs track '*.bin' '*.zip'"
        echo "  4. 重设暂存并使用 LFS: git reset HEAD <file> && git lfs checkout"
    fi
    exit 1
fi

# 5. 单元测试（可配置严格程度）
if [ -d "tests/" ] || grep -q "\[dev-dependencies\]" crates/*/Cargo.toml 2>/dev/null; then
    echo "运行单元测试..."
    TEST_OUTPUT=$(timeout 60 cargo test --lib --quiet 2>&1)
    TEST_EXIT=$?
    
    case $TEST_EXIT in
        0)
            echo -e "${GREEN}✓ 所有测试通过${NC}"
            ;;
        124)
            echo -e "${YELLOW}⚠ 警告: 测试超时 (60s)${NC}"
            if [ "$STRICT_TEST" -eq 1 ]; then
                echo "提示: 使用 'STRICT_TEST=0 git commit' 跳过此检查。"
                exit 1
            fi
            ;;
        *)
            if [ "$STRICT_TEST" -eq 1 ]; then
                echo -e "${RED}错误: 单元测试失败（STRICT_TEST=1）${NC}"
                echo "$TEST_OUTPUT" | tail -20
                exit 1
            else
                echo -e "${YELLOW}⚠ 警告: 部分测试失败（可选）${NC}"
                echo "提示: 使用 'STRICT_TEST=1 git commit' 强制通过。"
            fi
            ;;
    esac
fi

# 5.5 Markdown 文件质量检查
if [ "$CHECK_MARKDOWN" -eq 1 ]; then
    echo "检查暂存的 Markdown 文件..."
    MD_FILES=$(git diff --cached --name-only --diff-filter=ACM | grep '\.md$' || true)
    if [ ! -z "$MD_FILES" ]; then
        FAIL_MD=0
        while read -r file; do
            # 基础检查：检查是否包含空链接 []() 或未闭合的标签
            if grep -q "\[\]()" "$file"; then
                echo -e "${YELLOW}⚠ 警告: $file 包含空链接 []()${NC}"
                FAIL_MD=1
            fi
            # 检查是否有 TODO 标记
            if grep -iq "TODO" "$file"; then
                echo -e "${YELLOW}ℹ 提示: $file 包含 TODO 标记${NC}"
            fi
        done <<< "$MD_FILES"
        
        # 如果安装了 mdbook，则验证 SUMMARY.md 链接有效性
        if command -v mdbook &> /dev/null && [ -f "docs/book.toml" ]; then
            echo "执行 mdbook build 验证文档一致性..."
            if ! mdbook build docs > /dev/null 2>&1; then
                echo -e "${RED}错误: mdbook 构建失败，请检查 docs/SUMMARY.md 链接${NC}"
                exit 1
            fi
        fi
    fi
fi

# 6. 可选: Android 交叉编译预检 (Host Side Check)
# 智能提示：如果核心模块发生变动且未开启检查，给出建议
CHANGED_CORE=$(git diff --cached --name-only | grep -E '^crates/(aios-kernel|aios-adapter|aios-core)/' || true)
if [ ! -z "$CHANGED_CORE" ] && [ "$CHECK_ANDROID_TARGET" -eq 0 ]; then
    echo -e "${YELLOW}ℹ 智能提示: 检测到核心模块变动，建议开启交叉编译预检：${NC}"
    echo "  CHECK_ANDROID_TARGET=1 git commit ..."
fi

if [ "$CHECK_ANDROID_TARGET" -eq 1 ]; then
    echo "执行 Android 交叉编译预检 (target: aarch64-linux-android)..."
    if ! cargo check --target aarch64-linux-android -p aios-core --quiet; then
        echo -e "${RED}错误: 交叉编译检查失败！核心模块 aios-core 包含平台不兼容的代码。${NC}"
        echo "提示: 请检查是否误用了标准库中不支持 Android 的 API。"
        exit 1
    fi
    echo -e "${GREEN}✓ 交叉编译预检通过${NC}"
fi

echo -e "${GREEN}检查通过，允许提交！${NC}"
exit 0