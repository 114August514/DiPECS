# Shell 编码规范

> 目标：写出可移植、可维护、能在 CI 中自动验证的 Shell 脚本。
> 每条规则都有 **Why** 和 **Example**，而非罗列条款。

---

## 1. 总则

### 1.1 Shell 也是一种编程语言

Shell 不是"随便写写的胶水"——它和 Rust、Kotlin 一样，需要经过 code review、lint 检查、自动化测试。本项目中的 Shell 脚本承担了构建、环境配置、Git Hook、CI 检查等关键流程，它们和 `Cargo.toml` 同等重要。

### 1.2 防御性编程是默认姿态

Shell 的执行模型极其宽松——命令静默失败、变量未定义不报错、管道中间环节挂掉也不停。这种"宽容"在生产环境中是风险的温床。本规范的核心哲学是：**让脚本在任何异常情况下立即、响亮地失败**。

### 1.3 一致性优于个人偏好

如果你在改一个已有脚本（如 `scripts/dev/check-all.sh`），请保持和周围代码一致的风格。如果你发现整个项目的 Shell 脚本都不一致，提一个单独的 cleanup commit。

---

## 2. 解释器与执行环境

### 2.1 统一使用 `#!/usr/bin/env bash`

```bash
#!/usr/bin/env bash
```

**Why**: `#!/bin/bash` 假设 bash 的安装路径固定，这在 macOS（Homebrew 安装的 bash 在 `/opt/homebrew/bin/bash`）和某些 Linux 发行版上不成立。`/usr/bin/env` 从 `PATH` 中查找 bash，可移植性最好。

### 2.2 禁止使用的解释器

| 禁止 | 原因 |
| --- | --- |
| `#!/bin/sh` | POSIX sh 缺少数组、`[[`、`local`、`${var/replace}` 等关键特性 |
| `#!/bin/zsh` | 不是所有 CI 镜像都预装 zsh |
| `#!/usr/bin/env sh` | 同上，指向的是 dash/ash/busybox sh，功能受限 |

### 2.3 何时可以不用 bash

如果脚本真的只有 5 行且仅使用 POSIX 特性，可以用 `#!/bin/sh`——但前提是通过了 `checkbashisms` 工具的检查。本项目中，**默认用 bash**。

---

## 3. 严格模式与错误处理

### 3.1 强制启用严格模式

每个脚本必须在 shebang 之后、任何业务逻辑之前声明：

```bash
set -euo pipefail
```

三个选项的含义：

| 选项 | 作用 | 防止的问题 |
| --- | --- | --- |
| `-e` | 任何命令返回非零退出码时立即退出 | `rm` 失败后继续写入，误以为操作成功 |
| `-u` | 引用未定义变量时退出 | 拼写错误的变量被静默展开为空 |
| `-o pipefail` | 管道中任一命令失败则整体失败 | `grep pattern \| wc -l`——grep 失败时 wc 仍然成功 |

**Why**: bash 默认在命令失败时继续执行。这三个标志合在一起，把 bash 的容错模型从"静默吞掉错误"变成"爆炸式失败"——这才是适合 CI 和构建脚本的行为。

```bash
# 差：grep 失败时脚本继续执行，结果不可预测
grep "key" config.txt | awk '{print $2}' > output.txt

# 好：grep 失败时脚本立即退出
set -euo pipefail
grep "key" config.txt | awk '{print $2}' > output.txt
```

### 3.2 `set -e` 的已知陷阱

`set -e` 在以下场景中**不会**触发退出，你需要显式处理：

```bash
# 陷阱 1：if/while 条件中的命令不受 -e 影响
if command_that_fails; then  # 即使失败也不会退出
    echo "won't reach"
fi

# 陷阱 2：管道最右侧命令不受 -e 影响（-o pipefail 解决）
failing_cmd | true  # 即使 failing_cmd 失败，管道整体返回 0

# 陷阱 3：$(...) 命令替换中的命令不受 -e 影响
result=$(failing_cmd)  # 不会退出，result 为空

# 陷阱 4：&& 或 || 左侧的命令不受 -e 影响
failing_cmd && echo "won't reach"  # 不会退出

# 正确做法：在命令替换后显式检查
result=$(failing_cmd) || exit 1
```

### 3.3 错误信息必须可操作

每一条错误信息必须回答三个问题：**什么失败了？为什么？下一步怎么做？**

```bash
# 差：用户不知道是什么命令找不到
echo "Error: command not found"
exit 1

# 好
echo "[DiPECS] 错误: 找不到 'cargo' 命令。" >&2
echo "请安装 Rust: https://rustup.rs" >&2
exit 1
```

### 3.4 使用 `trap` 清理临时资源

```bash
readonly TMP_DIR=$(mktemp -d)

cleanup() {
    local exit_code=$?
    rm -rf "${TMP_DIR}"
    exit ${exit_code}
}
trap cleanup EXIT INT TERM
```

**Why**: 脚本可能在任意位置被中断（Ctrl+C、超时 kill、`set -e` 触发退出）。`trap EXIT` 保证清理代码无论如何都会执行。注意在 cleanup 中保留原始 exit code，避免掩盖真实错误。

---

## 4. 命名

| 元素 | 约定 | 示例 |
| --- | --- | --- |
| 全局变量/环境变量（export） | `SCREAMING_SNAKE_CASE` | `ANDROID_NDK_HOME`, `CARGO_TARGET_LINKER` |
| 脚本级只读常量 | `readonly` + `SCREAMING_SNAKE_CASE` | `readonly MAX_RETRIES=3` |
| 局部变量 | `snake_case` | `local target_arch`, `local exit_code` |
| 函数名 | `snake_case` | `check_dependencies`, `build_android` |
| 配置变量（用户可覆盖） | `SCREAMING_SNAKE_CASE` + `:-default` | `SKIP_CLIPPY=${SKIP_CLIPPY:-0}` |

### 4.1 变量命名要具体

```bash
# 差
local f=$(basename "$1")
local t="$2"

# 好
local binary_name=$(basename "${1}")
local dest_dir="${2}"
```

### 4.2 全局变量必须用 `readonly` 声明

```bash
# 差：全局变量可被意外篡改
MAX_RETRIES=3
TARGET="aarch64-linux-android"

# 好
readonly MAX_RETRIES=3
readonly TARGET="aarch64-linux-android"
```

### 4.3 缩写规则

- 极通用缩写可以用：`tmp`、`pid`、`src`、`dest`、`cmd`、`arg`
- 避免自造的模棱两可缩写：`bin_nm`（请写 `binary_name`）、`cfg_pth`（请写 `config_path`）

---

## 5. 格式化

### 5.1 缩进与空白

- **缩进 2 空格**（不用 Tab）
- 函数内部不额外缩进一级（函数体顶格）
- 管道接续使用 `|` 放在行尾，续行缩进 2 空格

```bash
# 好
build_android() {
  cargo build \
    --target aarch64-linux-android \
    --release \
    --package aios-daemon 2>&1 | grep -E "error|warning"
}
```

### 5.2 行宽

- 逻辑行不超过 100 字符
- 超长命令使用 `\` 折行，续行缩进 2 空格
- 超长管道使用 `|` 在行尾自然折行

```bash
# 好：长参数列表折行
cargo clippy \
  --workspace \
  --all-targets \
  --target aarch64-linux-android \
  -- -D warnings

# 好：长管道折行
git diff --cached --name-only --diff-filter=ACM \
  | grep '\.rs$' \
  | xargs rustfmt --check
```

### 5.3 函数间空行

函数之间空一行。逻辑段落之间空一行（如同样的操作组前后）。

---

## 6. 代码组织

### 6.1 脚本结构模板

```bash
#!/usr/bin/env bash
# scripts/<name>.sh — <一句话职责>
#
# 用法: scripts/<name>.sh [options]

# ---- 严格模式 ----
set -euo pipefail

# ---- 常量 ----
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# ---- 颜色（可选） ----
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly NC='\033[0m'  # No Color

# ---- 函数定义 ----
check_dependencies() { ... }
main() { ... }

# ---- 入口 ----
main "$@"
```

**Why 这个顺序**: 严格模式在最前面（任何逻辑执行前生效），常量在前（函数可能引用它们），入口在最后（所有函数已定义）。

### 6.2 一个脚本只做一件事

```text
scripts/
├── check-all.sh       # 全量 CI 检查（编排，调用 Rust 工具链）
├── setup-env.sh       # 环境配置（source 使用，设置 NDK 变量）
├── pre-commit.sh      # Git pre-commit hook
├── commit-msg.sh      # Git commit-msg hook
├── android-runner.sh  # Android 设备推送与运行
```

**Why**: 一个脚本一个职责。`check-all.sh` 负责"运行检查"，`setup-env.sh` 负责"设置环境"。两者分离后，CI 可以单独调用检查，开发者可以单独 source 环境配置。

### 6.3 Source vs Execute 的区分

脚本分为两类，必须在文件头部注明：

| 类型 | 用途 | 特征 | 示例 |
| --- | --- | --- | --- |
| **可执行脚本** | 独立运行（CI、git hook） | 以 `exit` 结束，可被 `./script.sh` 调用 | `check-all.sh` |
| **Source 脚本** | 被 `source` 引入（修改当前 shell 环境） | 以 `return` 处理错误，不做破坏性操作 | `setup-env.sh` |

```bash
# 在 source 脚本末尾加自检（项目惯例，来自 setup-env.sh）
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    echo "[DiPECS] 请使用 source 运行此脚本: source ${BASH_SOURCE[0]}" >&2
    exit 1
fi
```

### 6.4 函数粒度

- 每个函数不超过 30 行
- 函数名应描述**做什么**而非**怎么做**：`check_dependencies` > `check_cargo_and_git_installed`
- 如果发现自己在函数内部写 `# 步骤 1`、`# 步骤 2` 注释——应该把它们拆成独立函数

---

## 7. 变量与引用

### 7.1 引用的铁律

```bash
# 始终用双引号包裹变量引用
echo "${file_path}"     # 好

# 禁止裸变量引用（空格和通配符会导致单词拆分和路径展开）
echo $file_path         # 差：file_path="my file.txt" 时变成两个参数

# 例外：[[ ]] 内不需要引号包裹变量
[[ ${var} == "value" ]]  # 好，[[ ]] 内部不进行单词拆分
```

### 7.2 花括号

变量引用统一加花括号：`${var}` 而非 `$var`。唯一的例外是位置参数 `$1`、`$2`、`$@`、`$*`（但 `${1}` 也可以接受）。

```bash
# 好
local dest_dir="${PROJECT_ROOT}/target/${target_arch}"
echo "Building ${package_name} for ${target_arch}"

# 差
local dest_dir="$PROJECT_ROOT/target/$target_arch"
```

### 7.3 默认值与参数展开

| 语法 | 含义 | 适用场景 |
| --- | --- | --- |
| `${var:-default}` | var 未设置或为空时用 default | 可配置的阈值/开关 |
| `${var:=default}` | 同上，同时赋值给 var | 需要持久化默认值的场景 |
| `${var:?error_msg}` | var 未设置或为空时打印 error_msg 并退出 | 必填环境变量 |
| `${var:+value}` | var 已设置且非空时使用 value | 条件性添加参数 |

```bash
# 项目中的典型用法
SKIP_CLIPPY=${SKIP_CLIPPY:-0}              # 默认不跳过
TIMEOUT_CLIPPY=${TIMEOUT_CLIPPY:-120}      # 默认 120 秒
ADB_CMD="adb ${ADB_SERIAL:+-s ${ADB_SERIAL}}"  # 有序列号时追加 -s 参数
```

### 7.4 `local` 关键字

所有函数内变量必须用 `local` 声明：

```bash
# 好
build_android() {
    local target="${1}"
    local output_dir="${PROJECT_ROOT}/out"
    # ...
}

# 差：变量泄露到全局作用域
build_android() {
    target="${1}"
    output_dir="${PROJECT_ROOT}/out"
}
```

**Why**: bash 的变量默认是全局作用域。如果函数 A 调用函数 B，B 内部修改了 `target`，A 的 `target` 也被改了——这类 bug 极难排查。`local` 是唯一的防御手段。

### 7.5 数组

```bash
# 声明
local -a rust_files=()
local -A config_map=()  # 关联数组（bash 4.0+，本项目适用）

# 追加
rust_files+=("${file}")

# 遍历
for file in "${rust_files[@]}"; do
    echo "${file}"
done

# 注意："${array[@]}" 带引号，防止元素内有空格时拆分
# "${array[*]}" 把所有元素拼成一个字符串，通常不是你要的
```

---

## 8. 函数

### 8.1 函数定义风格

```bash
# 本项目统一使用此风格
function_name() {
    # 函数体
}

# 禁止使用
function function_name { ... }  # 关键词冗余，且 ksh 不兼容
```

### 8.2 参数接收

```bash
main() {
    local binary_path="${1}"
    local output_dir="${2:-.}"    # 第二个参数可选，默认当前目录
    local verbose="${3:-false}"

    if [[ -z "${binary_path}" ]]; then
        echo "用法: $0 <binary-path> [output-dir]" >&2
        exit 1
    fi
}
```

### 8.3 返回值

函数通过 `echo` 输出数据，通过 `return` 返回状态码（0 = 成功，非 0 = 失败）：

```bash
get_linker_name() {
    local api_level="${1}"
    echo "aarch64-linux-android${api_level}-clang"
    return 0
}

# 调用者
local linker=$(get_linker_name 33)
```

**Why**: bash 的 `return` 只能返回 0-255 的整数。用 stdout 传递数据是唯一的方式。但是——**不要往 stdout 输出调试信息**（见 8.4）。

### 8.4 日志与输出分离

数据用 `echo`（stdout），日志/错误用 `echo ... >&2`（stderr）：

```bash
# 好
check_linker() {
    local linker="${1}"
    if ! command -v "${linker}" &>/dev/null; then
        echo "[DiPECS] 错误: 未找到链接器 ${linker}" >&2
        return 1
    fi
    echo "${linker}"  # 输出数据到 stdout
}
```

**Why**: 如果你的函数既 echo 数据又 echo 日志，`$(check_linker)` 的调用者会同时拿到两者。

---

## 9. 条件与流程控制

### 9.1 `[[ ]]` > `[ ]`

```bash
# 好：[[ ]] 功能更强、更安全
if [[ "${var}" == "value" ]]; then
if [[ -f "${file}" && -s "${file}" ]]; then
if [[ "${str}" =~ ^[a-z]+$ ]]; then

# 差：[ ] 是外部命令，受单词拆分影响
if [ "${var}" == "value" ]; then  # 需要在 == 前后加空格，（还是对的有误导性——实际上 [ ] 应该是 = 而非 ==）
```

**Why**: `[[ ]]` 是 bash 内置语法，支持 `&&`/`||`、`=~` 正则匹配、不需要引号包裹变量。`[ ]` 是 `/usr/bin/[` 的外部调用（在某些系统上），语义上有诸多边界情况。

### 9.2 字符串比较

```bash
# 相等/不等
[[ "${var}" == "value" ]]
[[ "${var}" != "value" ]]

# 空/非空
[[ -z "${var}" ]]   # 为空
[[ -n "${var}" ]]   # 非空

# 正则匹配
[[ "${commit_msg}" =~ ^(feat|fix|docs|refactor|chore) ]]

# 通配符匹配
[[ "${file}" == *.rs ]]
```

### 9.3 数值比较

```bash
# 使用 (( )) 或 -eq/-gt/-lt
if (( exit_code != 0 )); then
if [[ "${exit_code}" -eq 0 ]]; then
if (( size > 1048576 )); then
```

### 9.4 `case` 用于多分支

```bash
case "${target}" in
    aarch64-linux-android)
        linker="aarch64-linux-android33-clang"
        ;;
    x86_64-unknown-linux-gnu)
        linker="cc"
        ;;
    *)
        echo "不支持的编译目标: ${target}" >&2
        exit 1
        ;;
esac
```

`case` 比 `if-elif-else` 链更清晰，且支持 `*` 通配。

---

## 10. 管道与命令替换

### 10.1 使用 `$(...)` 而非反引号

```bash
# 好
local result=$(command)

# 差
local result=`command`
```

**Why**: `$(...)` 支持嵌套（`$(echo $(pwd))`）、与引号交互更可预测、视觉上更清晰。

### 10.2 逐行处理使用 `while read`

```bash
# 好：安全处理带空格的文件名
git diff --cached --name-only --diff-filter=ACM \
  | while IFS= read -r file; do
      if [[ "${file}" == *.rs ]]; then
          echo "Rust file: ${file}"
      fi
  done

# 差：for 循环会把空格当分隔符
for file in $(git diff --cached --name-only); do  # 文件名含空格时截断
    ...
done
```

**Why**: `for` 循环在命令替换上执行单词拆分，文件名 `my file.rs` 会变成 `my` 和 `file.rs` 两个迭代。`while read` 配合 `IFS=` 和 `-r` 正确处理换行分隔。

### 10.3 进程替换 `<(...)`

```bash
# 当需要在一个子 shell 之外保留变量时（项目实际用法）
declare -a BIG_FILES
while IFS= read -r file; do
    local size=$(git cat-file -s ":${file}" 2>/dev/null || echo 0)
    if (( size > 1048576 )); then
        BIG_FILES+=("${file}")
    fi
done < <(git diff --cached --name-only --diff-filter=ACM)
```

**Why**: `cmd | while read ...` 创建了一个管道——`while` 体在子 shell 中运行，其中的变量赋值在管道结束后丢失。`done < <(cmd)` 使用进程替换避免了子 shell，变量可以保留到循环外部。

### 10.4 静默输出

```bash
# 只关心退出码，不关心输出时
if command -v cargo &>/dev/null; then
if cargo check --quiet 2>&1; then

# &>/dev/null: 同时重定向 stdout 和 stderr
# > /dev/null 2>&1: 旧写法，等价但更冗长
```

---

## 11. 字符串与文本处理

### 11.1 变量内字符串操作

```bash
local binary_path="/data/local/tmp/dipecsd"
local file_ext="${binary_path##*.}"      # 移除最长前缀：dipecsd
local file_name="${binary_path##*/}"     # 移除最长前缀（目录）：dipecsd
local dir_name="${binary_path%/*}"       # 移除最短后缀（文件）：/data/local/tmp
local without_ext="${binary_path%.*}"    # 移除最短后缀（扩展名）：/data/local/tmp/dipecs
```

### 11.2 使用 `printf` 进行复杂格式化

```bash
# 格式化数字（项目中用于显示文件大小）
printf '  %s (%.1fMB)\n' "${file}" "${size_mb}"

# 对齐表格
printf "%-20s %s\n" "Rustc:" "$(rustc --version)"
printf "%-20s %s\n" "Cargo:" "$(cargo --version)"
```

**Why**: `echo` 在不同 shell 和系统中的行为不一致（`echo -e` vs `echo`、转义字符处理）。`printf` 提供一致的格式化输出。

### 11.3 `awk` 提取列

```bash
# 从版本输出中提取版本号
rustc_version=$(rustc --version | awk '{print $2}')   # 输出 "1.95.0"
adb_version=$(adb version | head -n1 | awk '{print $5}')
```

---

## 12. I/O 与文件操作

### 12.1 检查文件/目录存在性

```bash
[[ -f "${file}" ]]     # 是常规文件
[[ -d "${dir}" ]]      # 是目录
[[ -s "${file}" ]]     # 文件存在且非空
[[ -x "${file}" ]]     # 文件可执行
[[ -f "${file}" && -s "${file}" ]]  # 文件存在且有内容
```

### 12.2 临时文件使用 `mktemp`

```bash
# 禁止：固定路径 + 可预测文件名
TMPFILE="/tmp/build_output.txt"

# 好：mktemp 保证唯一性和权限安全
readonly TMPFILE=$(mktemp) || exit 1
trap 'rm -f "${TMPFILE}"' EXIT

# 创建临时目录
readonly TMPDIR=$(mktemp -d) || exit 1
trap 'rm -rf "${TMPDIR}"' EXIT
```

**Why**: `/tmp/build_output.txt` 在并发执行时会互相覆盖，且可预测的文件名可能被符号链接攻击。`mktemp` 生成唯一文件名，且默认权限为 `0600`。

### 12.3 读取文件

```bash
# 读取整个文件到变量
local content=$(< "${file}")

# 逐行读取
while IFS= read -r line; do
    echo "> ${line}"
done < "${file}"
```

### 12.4 路径拼接

```bash
# 好：直接字符串拼接
local dest="${PROJECT_ROOT}/target/${target_arch}"
local hook_dir="${GIT_DIR}/hooks"

# 避免：dirname/basename 链式调用
local parent=$(dirname $(dirname $(dirname "${path}")))  # 难读且易错
```

---

## 13. 子进程与并发

### 13.1 `source` vs `bash`

```bash
source scripts/dev/setup-env.sh   # 在当前 shell 中执行——变量和 cd 会影响当前环境

bash scripts/dev/check-all.sh     # 在新子 shell 中执行——变量不会污染当前环境
```

**Why**: 要在当前 shell 中设置环境变量（如 `ANDROID_NDK_HOME`、`PATH`），必须 `source`。独立运行的脚本用 `bash` 调用，避免副作用。

### 13.2 并发任务

```bash
# 简单的并行等待
cargo build --package aios-core &
cargo build --package aios-daemon &
wait  # 等待所有后台任务完成
```

**注意**: 后台任务可能在脚本退出后继续运行。如果需要确保清理，使用 `trap` + `kill`。

---

## 14. 调试

### 14.1 执行追踪

```bash
# 开发阶段：追踪每一条执行的命令
set -x

# 仅追踪特定函数
main() {
    set -x
    # 调试代码
    set +x
}
```

### 14.2 调试变量

```bash
# 打印变量名和值
echo "SKIP_CLIPPY=${SKIP_CLIPPY}" >&2
echo "TOOLCHAIN=${TOOLCHAIN}" >&2
```

### 14.3 ShellCheck 内联抑制

```bash
# 仅在极少数场景下抑制特定警告，并加注释说明原因
# shellcheck disable=SC1090  # source 路径由运行时确定，无法静态分析
source "${runtime_path}"
```

**Why**: 不加注释的 `disable` 会掩盖真实 bug。每次抑制都必须在同一行注释解释原因。

---

## 15. 安全

### 15.1 路径注入防护

```bash
# 差：文件名可能包含空格、特殊字符
rm $1

# 好：引号包裹
rm "${1}"

# 好：限制文件操作范围
[[ "${1}" == *.rs ]] || { echo "仅允许 .rs 文件" >&2; exit 1; }
```

### 15.2 破坏性操作的确认

```bash
rm_rf_with_confirm() {
    local target="${1}"
    echo "即将删除: ${target}" >&2
    echo -n "确认? (y/N) " >&2
    read -r confirm
    [[ "${confirm}" == "y" ]] || { echo "已取消" >&2; exit 0; }
    rm -rf "${target}"
}
```

### 15.3 敏感信息

- 绝不把密钥、token、密码硬编码在脚本中
- 使用环境变量传递敏感值
- 脚本中不要 `echo "${API_KEY}"` 到日志中

---

## 16. 测试

### 16.1 修改后必须执行

每个 Shell 脚本修改后，必须在本地执行一次验证。对于有破坏性操作的脚本（如 `rm -rf`），先在安全路径下测试。

### 16.2 ShellCheck 集成

```bash
# 项目级检查
shellcheck scripts/**/*.sh

# CI 中集成（待添加到 check-all.sh）
shellcheck -x scripts/*.sh
```

### 16.3 测试 sanity check

```bash
# 对 setup-env.sh 的快速验证
source scripts/dev/setup-env.sh && echo "OK: 环境变量已设置"

# 验证脚本语法（不实际执行）
bash -n scripts/dev/check-all.sh && echo "OK: 语法正确"
```

---

## 17. 提交前检查

### 17.1 检查清单

每次修改 Shell 脚本后：

```bash
# 1. 语法检查
bash -n scripts/*.sh

# 2. Lint 检查
shellcheck -x scripts/*.sh

# 3. 测试执行（如适用）
bash scripts/dev/check-all.sh
```

### 17.2 脚本被 Git Hook 引用时特别注意

`pre-commit.sh` 和 `commit-msg.sh` 通过符号链接注入到 `.git/hooks/`。修改这些脚本后，需要重新运行 `source scripts/dev/setup-env.sh` 来刷新链接（或手动 `chmod +x`）。

---

## 18. 快速决策速查表

| 场景 | 选择 |
| --- | --- |
| 如何退出脚本 | `exit 1`（错误）/ `exit 0`（正常），不要用 `exit`（无参数，等同于 exit 上一个命令的状态码） |
| 命令找不到 | `command -v cmd &>/dev/null`（不用 `which`——which 在某些系统上不设置退出码） |
| 字符串比较 | `[[ ${a} == "${b}" ]]`（不用 `[ ]`） |
| 数值比较 | `(( a > b ))` 或 `[[ ${a} -gt ${b} ]]` |
| 默认值 | `${var:-default}` |
| 检查变量非空 | `[[ -n ${var} ]]` |
| 检查变量为空 | `[[ -z ${var} ]]` |
| 判断文件存在 | `[[ -f ${path} ]]` |
| 判断目录存在 | `[[ -d ${path} ]]` |
| 打印到 stderr | `echo "消息" >&2` |
| 静默执行 | `cmd &>/dev/null` |
| 管道失败即停 | `set -o pipefail` |
| 临时文件 | `mktemp` / `mktemp -d` |
| 清理资源 | `trap cleanup EXIT INT TERM` |
| 变量引用 | `"${var}"`（默认） |
| 逐行读取 | `while IFS= read -r line; do ... done < <(cmd)` |
| 命令替换 | `$(cmd)`（不用反引号） |
| 字符串操作 | `${var#prefix}`, `${var%suffix}`, `${var/old/new}` |
| 多分支 | `case ... esac`（不用 if-elif-else 链） |
| 数组追加 | `arr+=("elem")` |
| 函数局部变量 | `local name=value` |
| 函数返回值 | `return 0`（状态码），`echo`（数据到 stdout） |
| Source-only 保护 | `[[ "${BASH_SOURCE[0]}" == "${0}" ]] && { echo "source me"; exit 1; }` |
| 错误信息 | 包含：什么失败了 + 为什么 + 下一步怎么做 |
| 调试 | `set -x` 临时开启执行追踪 |

---

## 参考资料

- [Google Shell Style Guide](https://google.github.io/styleguide/shellguide.html)
- [ShellCheck Wiki](https://www.shellcheck.net/wiki/)
- [Bash Hackers Wiki](https://wiki.bash-hackers.org/)
- [GNU Bash Manual](https://www.gnu.org/software/bash/manual/)
