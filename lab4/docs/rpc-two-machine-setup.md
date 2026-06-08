# llama.cpp RPC 双机操作手册

## 目标与角色

| 名称 | 运行环境 | 职责 |
| :--- | :--- | :--- |
| **主机** `main-host` | 当前 Linux 主机 | 保存 GGUF、运行 `llama-cli`/`llama-bench`/Rust 工具、保存结果 |
| **从机** `rpc-worker` | WSL2 Ubuntu、原生 Ubuntu 或 USTC Vlab | 运行 `rpc-server`，向主机提供 CPU 或 NVIDIA GPU 算力 |

数据流：

```text
Qwen3.5 GGUF
    |
    v
main-host: llama-cli / llama-bench / lab4-bench
    |
    | 局域网 TCP 50052，或 Tailscale VPN 直连 Vlab:50052
    | 传输模型张量和计算请求
    v
rpc-worker: rpc-server -> CPU 或 CUDA
```

`rpc-worker` 不需要单独下载 GGUF。模型位于 `main-host`，首次 RPC 加载时会通过网络发送所需张量；worker 使用 `--cache` 后会把张量缓存到本地。

可选部署路线：

| 路线 | 连接方式 | 适用场景 |
| :--- | :--- | :--- |
| WSL2 / 原生 Ubuntu | 主机直接连接 `WORKER_IP:50052` | 同一可信局域网、Tailscale 等受控网络 |
| USTC Vlab | 主机通过 Tailscale VPN 直连 Vlab 的 `50052` | 没有方便的第二台物理机，优先完成双节点功能验证 |

> `llama.cpp` RPC 本身没有认证和传输加密。局域网路线只能用于可信网络；Vlab 路线必须使用加密隧道（Tailscale VPN 或 SSH 隧道），不要申请公网端口映射，也不要把 RPC 端口直接暴露到公网。

---

## Part A：主机（main-host）操作

以下命令在当前 Linux 主机执行。

### A1. 仓库与模型

```bash
# 拉取仓库（若已 clone 则跳过）
git clone --recurse-submodules https://github.com/114August514/DiPECS.git
cd DiPECS

# 确认版本一致
git rev-parse HEAD
git -C lab4/third_party/llama.cpp rev-parse HEAD
# 当前 llama.cpp: c4a278d68efa17811006f2123a84081dac03fac7

# 下载模型
mkdir -p lab4/data/models
wget --show-progress -c \
  "https://huggingface.co/bartowski/Qwen_Qwen3.5-2B-GGUF/resolve/main/Qwen_Qwen3.5-2B-Q4_K_M.gguf?download=true" \
  -O lab4/data/models/qwen3.5-2b-q4_k_m.gguf

# 校验
sha256sum lab4/data/models/qwen3.5-2b-q4_k_m.gguf
# 应为: 57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8
```

### A2. 编译 llama.cpp

```bash
cd lab4/third_party/llama.cpp
rm -rf build

CC=clang CXX=clang++ cmake -B build -S . \
  -DCMAKE_BUILD_TYPE=Release \
  -DGGML_RPC=ON

CC=clang CXX=clang++ cmake --build build --config Release -j "$(nproc)"
```

验证：

```bash
./build/bin/llama-cli --version
./build/bin/llama-bench --help
./build/bin/rpc-server --help
```

### A3. 单机验证

在连从机之前，先确认单机能跑：

```bash
./build/bin/llama-cli \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  -p "用中文解释什么是操作系统页缓存。" \
  -n 64 \
  --threads 12 \
  --reasoning off \
  --reasoning-budget 0
```

### A4. 连接从机

等从机启动 `rpc-server` 后再执行。

先设置公共路径变量：

```bash
export MODEL_PATH="$PWD/../../data/models/qwen3.5-2b-q4_k_m.gguf"
export MAIN_BUILD_DIR="$PWD/build"
```

然后根据从机类型选择一种连接方式。

#### A4.1 WSL2 / 原生 Ubuntu 直连

把 `WORKER_IP` 换成从机的局域网或 Tailscale 地址。当前测试环境可使用：

```bash
export WORKER_IP="${WORKER_IP:-<VLAB_TAILSCALE_IP>}"
export RPC_PORT="50052"
export WORKER_ENDPOINT="${WORKER_IP}:${RPC_PORT}"
```

检查网络：

```bash
ping -c 5 "$WORKER_IP"
nc -vz "$WORKER_IP" "$RPC_PORT"
```

#### A4.2 USTC Vlab Tailscale 直连

先按照 [Part C](#part-custc-vlab-cpu-worker) 在 Vlab 启动 `rpc-server`。

Vlab 通过 Tailscale 加入你的 tailnet 后，获取其 Tailscale IP：

```bash
# 在 Vlab 上执行
tailscale ip -4
```

在主机实验终端设置：

```bash
# 替换为 Vlab 实际的 Tailscale IP
export VLAB_TS_IP="100.xxx.xxx.xxx"
export RPC_PORT="50052"
export WORKER_ENDPOINT="${VLAB_TS_IP}:${RPC_PORT}"
```

检查网络：

```bash
ping -c 5 "$VLAB_TS_IP"
nc -vz "$VLAB_TS_IP" "$RPC_PORT"
```

主机直接通过 Tailscale VPN 访问 Vlab 的 `50052` 端口，无需 SSH 隧道或本地端口转发。

#### A4.3 发现远端设备

两种路线都使用已经设置好的 `WORKER_ENDPOINT`：

```bash
test -n "$WORKER_ENDPOINT"
nc -vz "${WORKER_ENDPOINT%:*}" "${WORKER_ENDPOINT##*:}"
```

发现远端设备：

```bash
"$MAIN_BUILD_DIR/bin/llama-cli" \
  --rpc "$WORKER_ENDPOINT" \
  --list-devices
```

输出中必须出现带有 RPC endpoint 的远端 `CPU` 或 `CUDA0`。如果只有本地设备，先查附录故障排查。

### A5. RPC smoke

```bash
"$MAIN_BUILD_DIR/bin/llama-cli" \
  --model "$MODEL_PATH" \
  --prompt "用中文简要说明 RPC 推理为什么可能比单机更慢。" \
  --n-predict 64 \
  --threads 12 \
  --ctx-size 1024 \
  --batch-size 64 \
  --seed 42 \
  --temp 0.7 \
  --reasoning off \
  --reasoning-budget 0 \
  --n-gpu-layers all \
  --rpc "$WORKER_ENDPOINT" \
  --single-turn \
  --no-display-prompt \
  --simple-io \
  --show-timings \
  --color off
```

成功标准：退出码 0，worker 终端出现连接日志，输出包含回答和 timing。

> 首次运行约 1.4 GB 模型张量传输，明显慢于后续运行属于正常现象。

### A6. 正式实验

#### llama-bench 对照

单机组（本地 CPU）：

```bash
"$MAIN_BUILD_DIR/bin/llama-bench" \
  --model "$MODEL_PATH" \
  --threads 12 --batch-size 64 \
  --n-prompt 128 --n-gen 64 \
  --repetitions 3 --n-gpu-layers 0 \
  --output jsonl \
  > ../../data/results/rpc-single-bench-qwen35.jsonl
```

RPC 组（远端设备）：

```bash
"$MAIN_BUILD_DIR/bin/llama-bench" \
  --model "$MODEL_PATH" \
  --threads 12 --batch-size 64 \
  --n-prompt 128 --n-gen 64 \
  --repetitions 3 --n-gpu-layers 99 \
  --rpc "$WORKER_ENDPOINT" \
  --output jsonl \
  > ../../data/results/rpc-distributed-bench-qwen35.jsonl
```

#### Rust 质量对照

单机：

```bash
cargo run --manifest-path ../../Cargo.toml -p lab4-tools --bin lab4-bench -- \
  --prompts ../../data/prompts/quality-prompts.jsonl \
  --executable "$MAIN_BUILD_DIR/bin/llama-cli" \
  --model "$MODEL_PATH" \
  --output ../../data/results/rpc-single-quality-qwen35.jsonl \
  --mode rpc-single-qwen35 \
  --case-prefix rpc-single-qwen35 \
  --repetitions 3 \
  --arg=--threads --arg=12 \
  --arg=--ctx-size --arg=1024 \
  --arg=--batch-size --arg=64 \
  --arg=--seed --arg=42 \
  --arg=--temp --arg=0.7 \
  --arg=--reasoning --arg=off \
  --arg=--reasoning-budget --arg=0 \
  --arg=--n-gpu-layers --arg=0 \
  --arg=--single-turn --arg=--no-display-prompt \
  --arg=--simple-io --arg=--show-timings \
  --arg=--color --arg=off
```

RPC：

```bash
cargo run --manifest-path ../../Cargo.toml -p lab4-tools --bin lab4-bench -- \
  --prompts ../../data/prompts/quality-prompts.jsonl \
  --executable "$MAIN_BUILD_DIR/bin/llama-cli" \
  --model "$MODEL_PATH" \
  --output ../../data/results/rpc-distributed-quality-qwen35.jsonl \
  --mode rpc-distributed-qwen35 \
  --case-prefix rpc-distributed-qwen35 \
  --repetitions 3 \
  --rpc "$WORKER_ENDPOINT" \
  --arg=--threads --arg=12 \
  --arg=--ctx-size --arg=1024 \
  --arg=--batch-size --arg=64 \
  --arg=--seed --arg=42 \
  --arg=--temp --arg=0.7 \
  --arg=--reasoning --arg=off \
  --arg=--reasoning-budget --arg=0 \
  --arg=--n-gpu-layers --arg=all \
  --arg=--single-turn --arg=--no-display-prompt \
  --arg=--simple-io --arg=--show-timings \
  --arg=--color --arg=off
```

汇总：

```bash
cargo run --manifest-path ../../Cargo.toml -p lab4-tools --bin lab4-summarize -- \
  ../../data/results/rpc-single-quality-qwen35.jsonl

cargo run --manifest-path ../../Cargo.toml -p lab4-tools --bin lab4-summarize -- \
  ../../data/results/rpc-distributed-quality-qwen35.jsonl
```

---

## Part B：从机（rpc-worker）操作

以下命令在 WSL2 Ubuntu 或原生 Ubuntu 执行。使用 Vlab 时跳到 [Part C](#part-custc-vlab-cpu-worker)。

### B1. 环境准备

```bash
sudo apt update
sudo apt install -y build-essential clang cmake git libcurl4-openssl-dev

clang --version
cmake --version
```

### B2. 拉取仓库

```bash
git clone --recurse-submodules https://github.com/114August514/DiPECS.git
cd DiPECS

# 填入主机上 `git rev-parse HEAD` 输出的完整 commit
export MAIN_REPO_COMMIT="<MAIN_REPO_COMMIT>"
export LLAMA_COMMIT="c4a278d68efa17811006f2123a84081dac03fac7"

git fetch origin
git switch --detach "$MAIN_REPO_COMMIT"
git submodule update --init --recursive

# 验证
test "$(git rev-parse HEAD)" = "$MAIN_REPO_COMMIT"
test "$(git -C lab4/third_party/llama.cpp rev-parse HEAD)" = "$LLAMA_COMMIT"
```

设置路径：

```bash
export REPO_ROOT="$PWD"
export RPC_PORT="50052"
```

### B3. CPU Worker（无 NVIDIA GPU）

构建：

```bash
export WORKER_CPU_BUILD_DIR="$REPO_ROOT/lab4/third_party/llama.cpp/build-rpc-cpu"

CC=clang CXX=clang++ cmake \
  -S "$REPO_ROOT/lab4/third_party/llama.cpp" \
  -B "$WORKER_CPU_BUILD_DIR" \
  -DCMAKE_BUILD_TYPE=Release \
  -DGGML_RPC=ON

cmake --build "$WORKER_CPU_BUILD_DIR" \
  --config Release \
  --target rpc-server \
  -j "$(nproc)"

"$WORKER_CPU_BUILD_DIR/bin/rpc-server" --help
```

启动（保持终端运行）：

```bash
"$WORKER_CPU_BUILD_DIR/bin/rpc-server" \
  --host 0.0.0.0 \
  --port "$RPC_PORT" \
  --threads "$(nproc)" \
  --cache
```

验证监听：

```bash
ss -ltn | grep ":${RPC_PORT}"
```

### B4. CUDA Worker（有 NVIDIA GPU）

前置检查：

```bash
nvidia-smi
nvcc --version
```

> Windows 侧先装支持 WSL 的 NVIDIA 驱动。WSL2 内只装 CUDA Toolkit，**不要**装 `cuda-drivers`。

构建（与 CPU 使用不同目录）：

```bash
export WORKER_CUDA_BUILD_DIR="$REPO_ROOT/lab4/third_party/llama.cpp/build-rpc-cuda"

cmake \
  -S "$REPO_ROOT/lab4/third_party/llama.cpp" \
  -B "$WORKER_CUDA_BUILD_DIR" \
  -DCMAKE_BUILD_TYPE=Release \
  -DGGML_CUDA=ON \
  -DGGML_RPC=ON

cmake --build "$WORKER_CUDA_BUILD_DIR" \
  --config Release \
  --target rpc-server \
  -j "$(nproc)"
```

启动：

```bash
"$WORKER_CUDA_BUILD_DIR/bin/rpc-server" \
  --host 0.0.0.0 \
  --port "$RPC_PORT" \
  --cache
```

日志应列出 `CUDA0` 和 GPU 名称。多 GPU 限制设备：

```bash
"$WORKER_CUDA_BUILD_DIR/bin/rpc-server" \
  --host 0.0.0.0 \
  --port "$RPC_PORT" \
  --device CUDA0 \
  --cache
```

不要同时启动 CPU 和 CUDA worker 占同一个端口。切换时先 `Ctrl+C` 停止当前服务。

### B5. WSL2 网络配置

先确认 WSL 版本：

```powershell
wsl --list --verbose
```

#### B5.1 推荐：Windows 11 镜像网络

在 `$env:USERPROFILE\.wslconfig` 写入：

```ini
[wsl2]
networkingMode=mirrored
```

保存后：

```powershell
wsl --shutdown
```

重新进入 Ubuntu 启动 `rpc-server`。然后管理员 PowerShell 开防火墙：

```powershell
New-NetFirewallHyperVRule `
  -Name "LlamaCppRpc50052" `
  -DisplayName "llama.cpp RPC 50052" `
  -Direction Inbound `
  -VMCreatorId "{40E0AC32-46A5-438A-A0B2-2B479E8F2E90}" `
  -Protocol TCP `
  -LocalPorts 50052
```

查询 Windows 局域网 IP 给主机用：

```powershell
Get-NetIPAddress -AddressFamily IPv4 |
  Where-Object {
    $_.InterfaceAlias -notmatch "Loopback|vEthernet" -and
    $_.IPAddress -notlike "169.254.*"
  } |
  Format-Table InterfaceAlias, IPAddress
```

#### B5.2 兼容：WSL2 默认 NAT

管理员 PowerShell：

```powershell
$WslIp = ((wsl.exe -d Ubuntu hostname -I).Trim() -split "\s+")[0]
$WindowsLanIp = "192.168.1.20"

netsh interface portproxy add v4tov4 `
  listenaddress=$WindowsLanIp `
  listenport=50052 `
  connectaddress=$WslIp `
  connectport=50052

New-NetFirewallRule `
  -DisplayName "llama.cpp RPC 50052" `
  -Direction Inbound -Action Allow `
  -Protocol TCP -LocalPort 50052 `
  -Profile Private

# 验证
netsh interface portproxy show all
```

主机连接 `$WindowsLanIp`，不是 NAT 后面的 `$WslIp`。WSL 重启 IP 会变，需重建 `portproxy`。

#### B5.3 原生 Ubuntu

不需要 Windows 转发。查 IP：

```bash
hostname -I
```

UFW 放行（可选）：

```bash
export MAIN_HOST_IP="192.168.1.10"
sudo ufw allow from "$MAIN_HOST_IP" to any port 50052 proto tcp
```

---

## Part C：USTC Vlab CPU Worker

Vlab 默认提供 2 核 CPU、6 GB 内存和 16 GB 磁盘。当前实例约有 4.8 GB 可用内存、4.2 GB 可用磁盘，足以完成 Qwen3.5-2B Q4_K_M 的 CPU RPC 冒烟与对照实验，但不适合作为加速节点。

Vlab 是远程 LXC 计算环境，课程是否将其计作严格意义上的”第二台机器”应向助教确认。实验报告必须明确写出 `USTC Vlab LXC + Tailscale VPN + CPU RPC`，不能将结果描述成裸 TCP 局域网测试。

### C1. 安装并登录 Tailscale

在 Vlab 上安装 Tailscale：

```bash
curl -fsSL https://tailscale.com/install.sh | sh
```

启动并登录：

```bash
sudo tailscale up
```

执行后会输出一个登录链接（如 `https://login.tailscale.com/a/xxxx`），在浏览器中打开并授权。

验证状态并获取 Tailscale IP：

```bash
tailscale status
tailscale ip -4
```

记下输出的 IPv4 地址（形如 `100.xxx.xxx.xxx`），后续主机连接时需要。

> 如果 `tailscale up` 报错涉及 `/dev/net/tun`，说明 Vlab LXC 容器缺少 TUN 设备权限。尝试：
> ```bash
> sudo mkdir -p /dev/net
> sudo mknod /dev/net/tun c 10 200
> sudo chmod 600 /dev/net/tun
> sudo tailscale up
> ```
> 若仍失败，则 Vlab 不支持 Tailscale，需回退到 SSH 隧道方案。

### C2. 检查资源并安装构建工具

在 Vlab 终端中检查架构、CPU、内存和磁盘：

```bash
uname -m
nproc
free -h
df -h /
```

本手册只支持 `x86_64` Vlab CPU Worker。构建前建议至少保留 3 GB 可用磁盘和 3 GB 可用内存。

安装最小工具链：

```bash
sudo apt update
sudo apt install -y build-essential cmake ninja-build git

g++ --version
cmake --version
ninja --version
```

这里使用 Ninja 生成器，不新增或维护课程代码的 Makefile。上游 `llama.cpp` 源码中自带的 Makefile 属于未修改的第三方文件，本实验不使用它们。

### C3. 精简拉取固定版本的 llama.cpp

Vlab 只运行 `rpc-server`，不需要复制整个 DiPECS 仓库，也不需要上传约 1.4 GB 的 GGUF 模型。

```bash
export VLAB_WORK_ROOT="$HOME/lab4-rpc-worker"
export LLAMA_COMMIT="c4a278d68efa17811006f2123a84081dac03fac7"

mkdir -p "$VLAB_WORK_ROOT"
cd "$VLAB_WORK_ROOT"

git init llama.cpp
cd llama.cpp
git remote add origin https://github.com/ggml-org/llama.cpp.git
git fetch --depth 1 origin "$LLAMA_COMMIT"
git checkout --detach FETCH_HEAD

test "$(git rev-parse HEAD)" = "$LLAMA_COMMIT"
```

若目录已经存在，更新时执行：

```bash
cd "$HOME/lab4-rpc-worker/llama.cpp"
git fetch --depth 1 origin "$LLAMA_COMMIT"
git checkout --detach "$LLAMA_COMMIT"
test "$(git rev-parse HEAD)" = "$LLAMA_COMMIT"
```

主机和 Vlab 的 `llama.cpp` commit 必须完全一致，否则 RPC 协议可能不兼容。

### C4. 仅构建 CPU rpc-server

```bash
cd "$HOME/lab4-rpc-worker/llama.cpp"
export VLAB_RPC_BUILD_DIR="$PWD/build-rpc-cpu"

cmake -G Ninja \
  -S . \
  -B "$VLAB_RPC_BUILD_DIR" \
  -DCMAKE_BUILD_TYPE=Release \
  -DGGML_RPC=ON \
  -DLLAMA_BUILD_TOOLS=ON \
  -DLLAMA_BUILD_TESTS=OFF \
  -DLLAMA_BUILD_EXAMPLES=OFF \
  -DLLAMA_BUILD_SERVER=OFF \
  -DLLAMA_BUILD_APP=OFF

cmake --build "$VLAB_RPC_BUILD_DIR" \
  --target rpc-server \
  -j 2

"$VLAB_RPC_BUILD_DIR/bin/rpc-server" --help
du -sh "$HOME/lab4-rpc-worker"
df -h /
```

如果构建时磁盘不足，先删除旧构建目录和无关缓存，再重新配置；不要把 GGUF 上传到 Vlab。

### C5. 启动 Vlab Worker

Vlab 的 RPC 端口绑定到 `0.0.0.0`，由 Tailscale VPN 访问：

```bash
cd "$HOME/lab4-rpc-worker/llama.cpp"
export VLAB_RPC_BUILD_DIR="$PWD/build-rpc-cpu"
export RPC_PORT="50052"
export LLAMA_CACHE="$HOME/.cache/llama.cpp"

"$VLAB_RPC_BUILD_DIR/bin/rpc-server" \
  --host 0.0.0.0 \
  --port "$RPC_PORT" \
  --threads 2 \
  --cache
```

保持终端运行。在 Vlab 上验证监听范围：

```bash
ss -ltn | grep "0.0.0.0:${RPC_PORT}"
```

必须看到 `0.0.0.0:50052`。由于 Vlab 本身没有公网 IP，该端口仅通过 Tailscale 虚拟网络可达，不会暴露到公网。首次 RPC 加载后检查缓存和磁盘：

```bash
du -sh "$LLAMA_CACHE/rpc"
df -h /
```

缓存可能占用接近模型张量大小。如果剩余空间过低，停止 worker 后删除 RPC 缓存：

```bash
rm -rf "$LLAMA_CACHE/rpc"
```

### C6. 运行实验

1. Vlab 终端保持 `rpc-server` 运行。
2. 主机实验终端执行 [A4.2](#a42-ustc-vlab-tailscale-直连) 配置 `WORKER_ENDPOINT`。
3. 主机实验终端执行 [A4.3](#a43-发现远端设备) 和 [A5](#a5-rpc-smoke)。
4. 冒烟成功后执行 [A6](#a6-正式实验)。

Vlab 只有 2 核 CPU，且 RPC 流量经过 Tailscale VPN，因此它大概率比主机单机 CPU 更慢。这不是实验失败，应在报告中从以下角度解释：

- Vlab CPU 核数与单核性能受限；
- 模型张量首次通过公网传输；
- Tailscale WireGuard 加密、封装与额外数据复制；
- 公网 RTT、带宽与抖动；
- 主机和远端 CPU 之间的同步等待；
- `--cache` 启用前后模型加载时间差异。

---

## 附录

### 故障排查

| 现象 | 检查项 |
| :--- | :--- |
| `nc` 连不上 | 从机 `ss -ltn \| grep :50052`；确认 `--host 0.0.0.0`；检查防火墙 |
| WSL2 NAT 局域网不通 | 主机必须用 `$WindowsLanIp`，不是 WSL NAT IP；`netsh interface portproxy show all` |
| Tailscale 连接后 `nc` 不通 | 双方 `tailscale status` 确认 Online；Vlab 确认 `rpc-server` 监听 `0.0.0.0:50052`；主机确认 `tailscale ip -4` 与 Vlab 输出一致 |
| Tailscale `up` 报错 TUN | Vlab LXC 可能缺少 TUN 设备权限；回退 SSH 隧道方案 |
| 主机 `ping` 不通 Vlab Tailscale IP | 检查双方是否在同一 tailnet；确认 tailscale 授权完成；检查 Tailscale ACL 是否放行 |
| Vlab 构建或缓存时磁盘不足 | `df -h /`、`du -sh ~/lab4-rpc-worker ~/.cache/llama.cpp/rpc`；删除旧构建目录或停止服务后清理 RPC 缓存 |
| 能连端口但无 RPC 设备 | 两端 `git -C lab4/third_party/llama.cpp rev-parse HEAD` 必须一致；都必须 `-DGGML_RPC=ON` |
| CUDA worker 只显示 CPU | `nvidia-smi` + `nvcc --version`；确认 `-DGGML_CUDA=ON`；用 `build-rpc-cuda` 目录 |
| 首次运行长时间无输出 | 正常，1.4 GB 张量首次传输。观察 worker 日志，不要终止 |

排错时开启详细日志：

```bash
GGML_RPC_DEBUG=1 "$WORKER_CPU_BUILD_DIR/bin/rpc-server" \
  --host 0.0.0.0 --port "$RPC_PORT" \
  --threads "$(nproc)" --cache
```

### 清理

从机按 `Ctrl+C` 停止 `rpc-server`。

WSL2 NAT 删除转发：

```powershell
netsh interface portproxy delete v4tov4 `
  listenaddress=$WindowsLanIp listenport=50052
Remove-NetFirewallRule -DisplayName "llama.cpp RPC 50052"
```

镜像网络删除规则：

```powershell
Remove-NetFirewallHyperVRule -Name "LlamaCppRpc50052"
```

Vlab 路线：

- 在 Vlab worker 终端按 `Ctrl+C` 停止 `rpc-server`；
- 在 Vlab 上可选执行 `sudo tailscale down` 断开 VPN；
- 需要释放磁盘时，确认 worker 已停止，再删除 `$HOME/.cache/llama.cpp/rpc`。

### 实验记录清单

至少保存：

- 两端仓库 commit 与 `llama.cpp` commit
- 两端 CPU/内存/GPU/OS/内核
- 从机类型：WSL2、原生 Ubuntu 或 USTC Vlab LXC
- WSL 版本、NAT 或 mirrored；Vlab 则记录 Tailscale IP
- 网络类型（有线/Wi-Fi/热点/Vlab Tailscale VPN）
- `ping` RTT；若禁 ping 记录该情况
- worker 启动日志和远端设备列表
- 一次成功 RPC 推理截图
- 单机与 RPC 的 JSONL 原始数据
- 模型文件名、大小、SHA-256
- 首次传输与缓存后运行的差异

报告不要只写"更快"或"更慢"，要结合以下因素解释：

- TCP RTT 和带宽
- 模型张量首次传输
- CPU/GPU 算力差异
- 主机与 worker 同步等待
- 模型切分和 KV cache 放置
- Wi-Fi 抖动和后台任务

### 参考资料

- [llama.cpp RPC](https://github.com/ggml-org/llama.cpp/blob/c4a278d68efa17811006f2123a84081dac03fac7/tools/rpc/README.md)
- [Microsoft WSL 网络](https://learn.microsoft.com/windows/wsl/networking)
- [NVIDIA CUDA on WSL](https://docs.nvidia.com/cuda/wsl-user-guide/)
- [USTC Vlab SSH 命令行登录](https://vlab.ustc.edu.cn/docs/login/ssh/)
- [USTC Vlab 系统资源与端口转发](https://vlab.ustc.edu.cn/docs/advanced/resources/)
- [Tailscale 快速入门](https://tailscale.com/kb/1017/install)
- [Tailscale 在 LXC 容器中使用](https://tailscale.com/kb/1130/lxc-unprivileged)
- [Qwen3.5-2B Q4_K_M GGUF](https://huggingface.co/bartowski/Qwen_Qwen3.5-2B-GGUF/blob/main/Qwen_Qwen3.5-2B-Q4_K_M.gguf)
