# llama.cpp RPC 双机操作手册

## 目标与角色

| 名称 | 运行环境 | 职责 |
| :--- | :--- | :--- |
| **主机** `main-host` | 当前 Linux 主机 | 保存 GGUF、运行 `llama-cli`/`llama-bench`/Rust 工具、保存结果 |
| **从机** `rpc-worker` | WSL2 Ubuntu 或原生 Ubuntu | 运行 `rpc-server`，向主机提供 CPU 或 NVIDIA GPU 算力 |

数据流：

```text
Qwen3.5 GGUF
    |
    v
main-host: llama-cli / llama-bench / lab4-bench
    |
    | TCP 50052，传输模型张量和计算请求
    v
rpc-worker: rpc-server -> CPU 或 CUDA
```

`rpc-worker` 不需要单独下载 GGUF。模型位于 `main-host`，首次 RPC 加载时会通过网络发送所需张量；worker 使用 `--cache` 后会把张量缓存到本地。

> `llama.cpp` RPC 没有认证和传输加密，只在可信局域网中使用，不要将端口 50052 暴露到公网。

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

设置变量（把 `192.168.1.20` 换成从机实际 IP）：

```bash
export WORKER_IP="192.168.1.20"
export RPC_PORT="50052"
export WORKER_ENDPOINT="${WORKER_IP}:${RPC_PORT}"
export MODEL_PATH="$PWD/../../data/models/qwen3.5-2b-q4_k_m.gguf"
export MAIN_BUILD_DIR="$PWD"
```

网络检查：

```bash
ping -c 5 "$WORKER_IP"
nc -vz "$WORKER_IP" "$RPC_PORT"
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

以下命令在 WSL2 Ubuntu 或原生 Ubuntu 执行。

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

## 附录

### 故障排查

| 现象 | 检查项 |
| :--- | :--- |
| `nc` 连不上 | 从机 `ss -ltn \| grep :50052`；确认 `--host 0.0.0.0`；检查防火墙 |
| WSL2 NAT 局域网不通 | 主机必须用 `$WindowsLanIp`，不是 WSL NAT IP；`netsh interface portproxy show all` |
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

### 实验记录清单

至少保存：

- 两端仓库 commit 与 `llama.cpp` commit
- 两端 CPU/内存/GPU/OS/内核
- WSL 版本、NAT 或 mirrored
- 网络类型（有线/Wi-Fi/热点）
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
- [Qwen3.5-2B Q4_K_M GGUF](https://huggingface.co/bartowski/Qwen_Qwen3.5-2B-GGUF/blob/main/Qwen_Qwen3.5-2B-Q4_K_M.gguf)
