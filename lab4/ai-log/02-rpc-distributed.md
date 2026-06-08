# AI 审计日志：RPC 双机推理实验

## 实验目标

通过 Tailscale WireGuard VPN 连接本地主机与 USTC Vlab LXC，验证 llama.cpp RPC 分布式推理功能，测量网络传输与跨机计算开销。

## 工具与版本

| 工具 | 版本/来源 | 用途 |
| :--- | :--- | :--- |
| `rpc-server` | llama.cpp b9533-c4a278d68 (build-rpc-cpu) | 远端计算后端 |
| `llama-cli` | 同上 (build, -DGGML_RPC=ON) | RPC 客户端 |
| `llama-bench` | 同上 | RPC 性能基准 |
| `nc` | OpenBSD netcat | TCP 端口连通性探测 |
| `tailscale` | 用户 VPN 网络 | 主机与 Vlab 加密隧道 |
| `lab4-bench` (Rust) | lab4-tools crate | 批量质量测试（单机 vs RPC 对照） |

## 关键命令

### Vlab 端启动 rpc-server

```bash
./build-rpc-cpu/bin/rpc-server \
  --host 0.0.0.0 \
  --port 50052 \
  --threads 2
```

### 主机端连通性探测

```bash
nc -vz <VLAB_IP> 50052
# Connection to <VLAB_IP> 50052 port [tcp/*] succeeded!
```

### 设备发现（截图证据）

```bash
./build/bin/llama-cli \
  --rpc <VLAB_IP>:50052 \
  --list-devices
# Available devices:
#   RPC0: <VLAB_IP>:50052 (257565 MiB, 257565 MiB free)
```

### RPC 推理（截图证据）

```bash
./build/bin/llama-cli \
  -m data/models/qwen3.5-2b-q4_k_m.gguf \
  --rpc <VLAB_IP>:50052 \
  --ctx-size 1024 --batch-size 64 \
  --ubatch-size 64 --fit off \
  --threads 8 --n-predict 64 \
  --prompt "什么是RPC" \
  --reasoning off --reasoning-budget 0 \
  --single-turn --no-display-prompt \
  --simple-io --show-timings
```

### RPC 性能基准（llama-bench）

```bash
./build/bin/llama-bench \
  -m data/models/qwen3.5-2b-q4_k_m.gguf \
  --rpc <VLAB_IP>:50052 \
  -t 8 -p 128 -n 64
```

### 单机对照组

```bash
./build/bin/llama-bench \
  -m data/models/qwen3.5-2b-q4_k_m.gguf \
  -t 8 -p 128 -n 64
```

## 关注的指标/事件

| 指标 | 工具 | 系统含义 |
| :--- | :--- | :--- |
| Prompt throughput (pp t/s) | llama-bench | 跨机张量传输 + 远端 prefill |
| Generation throughput (tg t/s) | llama-bench | 逐 token 跨机 decode + 同步等待 |
| 端到端耗时 | Rust lab4-bench | 模型加载（含首次 RPC 张量传输）+ 推理 |
| 首次 RPC 模型加载时间 | 人工观察 | 约 1.4 GB 张量通过 Tailscale VPN 传输 |
| SIGKILL / OOM | gdb + dmesg | Vlab LXC cgroup 内存限制（默认 ctx-size 导致 KV cache 超限） |

## 关键结论

1. **RPC 推理成功，但性能显著低于单机**：Prompt 吞吐降至 ~12%（24.95 vs 213.48 t/s），Generation 降至 ~17%（4.85 vs 35.30 t/s）。
2. **瓶颈在从机资源**：Vlab 仅 2 vCPU（Intel Xeon Silver 4314），单核性能和多核并行能力均大幅落后于主机 i7-13700H。
3. **默认 ctx-size 导致服务端 OOM**：llama-cli 未指定 `--ctx-size` 时，服务端尝试分配模型训练上下文级别的 KV cache，触发 Vlab LXC 的 cgroup SIGKILL。**限制 `--ctx-size 1024` 后正常**。
4. **RPC 的价值在任务级并行**：单请求性能下降符合预期，RPC 用于将计算分发到多台机器，实现更大规模的并行。

## 证据位置

| 证据 | 路径 | 说明 |
| :--- | :--- | :--- |
| 设备发现截图 | `assets/02-rpc-device-discovery.png` | RPC0 设备列表（脱敏） |
| RPC 推理截图 | `assets/03-rpc-inference.png` | 中文回答与 3.3/4.8 t/s |
| RPC bench 原始数据 | `data/results/rpc-distributed-bench-qwen35.jsonl` | RPC 组 llama-bench 输出 |
| 单机对照数据 | `data/results/rpc-single-bench-qwen35.jsonl` | 单机组 llama-bench 输出 |
| RPC 质量测试 | `data/results/rpc-distributed-quality-qwen35.jsonl` | 15 条 prompt × RPC |
| 单机质量对照 | `data/results/rpc-single-quality-qwen35.jsonl` | 15 条 prompt × 单机 |
| 汇总报告 | `reports/rpc-experiment-report.md` | 完整环境、命令与分析 |
