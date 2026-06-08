# Lab4 RPC 双机实验报告

## 1. 实验环境

### 1.1 主机（main-host）

| 项目 | 内容 |
| :--- | :--- |
| 机器类型 | 本地物理机 |
| OS / 内核 | Linux archlinux 7.0.11-arch1-1 #1 SMP PREEMPT_DYNAMIC |
| CPU | 13th Gen Intel(R) Core(TM) i7-13700H |
| 内存 | 15 GiB total，4.8 GiB available |
| GPU | 无（CPU 推理） |
| Tailscale IP | `100.x.x.x`（已脱敏） |
| DiPECS 仓库 commit | `c713fe62af7e1e4ae0a1ebe1f7b2d1b3c6e5d294` |
| llama.cpp commit | `c4a278d68efa17811006f2123a84081dac03fac7` |

### 1.2 从机（rpc-worker）

| 项目 | 内容 |
| :--- | :--- |
| 机器类型 | USTC Vlab LXC |
| OS / 内核 | Ubuntu LXC / Linux 7.0.0-3-pve |
| CPU | Intel(R) Xeon(R) Silver 4314 CPU @ 2.40GHz |
| 分配给 LXC 的 vCPU | 2 |
| 内存 | 6.0 GiB total，4.1 GiB available |
| GPU | 无 |
| 磁盘 | 16G total，3.5G available（78% 已用） |
| Tailscale IP | `100.y.y.y`（已脱敏） |
| llama.cpp commit | `c4a278d68efa17811006f2123a84081dac03fac7` |

### 1.3 网络连接

| 项目 | 内容 |
| :--- | :--- |
| 连接方式 | Tailscale WireGuard VPN |
| 主机 Tailscale IP | `100.x.x.x`（已脱敏） |
| Vlab Tailscale IP | `100.y.y.y`（已脱敏） |
| `ping` RTT | ICMP 被禁，无法 ping 通；TCP 端口探测成功 |
| RPC 端口 | 50052 |
| 加密方式 | WireGuard（Tailscale） |

### 1.4 模型信息

| 项目 | 内容 |
| :--- | :--- |
| 文件名 | qwen3.5-2b-q4_k_m.gguf |
| 磁盘文件大小 | 1,396,198,496 bytes（约 1.30 GiB） |
| llama-bench 张量大小 | 1,385,235,712 bytes |
| SHA-256 | `57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8` |
| 量化格式 | Q4_K - Medium |
| 参数规模 | 约 1.94 B |

---

## 2. 关键命令与运行日志

### 2.1 Vlab 端启动 rpc-server

```bash
cd ~/lab4-rpc-worker/llama.cpp
export VLAB_RPC_BUILD_DIR="$PWD/build-rpc-cpu"
export RPC_PORT="50052"

"$VLAB_RPC_BUILD_DIR/bin/rpc-server" \
  --host 0.0.0.0 \
  --port "$RPC_PORT" \
  --threads 2 \
  --cache
```

关键输出：

```text
Starting RPC server v4.0.0
  endpoint       : 0.0.0.0:50052
  local cache    : /home/ubuntu/.cache/llama.cpp/rpc/
Devices:
  CPU: Intel(R) Xeon(R) Silver 4314 CPU @ 2.40GHz (257565 MiB, 257565 MiB free)
  transport      : TCP
```

### 2.2 主机端发现远端设备

```bash
export VLAB_TS_IP="<VLAB_TAILSCALE_IP>"
export RPC_PORT="50052"
export WORKER_ENDPOINT="${VLAB_TS_IP}:${RPC_PORT}"
nc -vz "$VLAB_TS_IP" "$RPC_PORT"
llama-cli --rpc "$WORKER_ENDPOINT" --list-devices
```

关键输出：

```text
Connection to <VLAB_TAILSCALE_IP> 50052 port [tcp/*] succeeded!
Available devices:
  RPC0: <VLAB_TAILSCALE_IP>:50052 (257565 MiB, 257565 MiB free)
```

---

## 3. 实验结果

### 3.1 llama-bench 性能对照

| 指标 | 单机 CPU | RPC (Vlab CPU) | 倍数 |
| :--- | ---: | ---: | ---: |
| Prompt 处理 (t/s) | 213.48 | 24.94 | **0.12x** |
| Token 生成 (t/s) | 34.45 | 5.80 | **0.17x** |

### 3.2 Rust 质量对照

| 指标 | 单机 CPU | RPC (Vlab CPU) | 倍数 |
| :--- | ---: | ---: | ---: |
| 记录数 | 15 | 15 | - |
| 成功率 | 15/15 (100%) | 15/15 (100%) | - |
| 平均耗时 | 8,469.67 ms | 118,392.80 ms | **14.0x** |
| 平均生成速度 | 31.84 t/s | 5.14 t/s | **0.16x** |

---

## 4. 结果分析

### 4.1 为什么 RPC 比单机慢

1. **Vlab CPU 资源严重受限**：Vlab 仅分配 2 vCPU，而主机 i7-13700H 为 14 核（6P+8E）20 线程，单核性能和多核并行能力均大幅领先。
2. **网络传输开销**：RPC 每次计算都需要通过 Tailscale VPN 在主机和 Vlab 之间传输张量与中间结果。
3. **模型加载开销**：`lab4-bench` 每个 case 独立启动 `llama-cli`，首次运行需将约 1.4 GB 模型张量传输到 Vlab（p001-r01 耗时约 120s）；启用 `--cache` 后缓存命中，但仍需重新建立连接和验证缓存。
4. **WireGuard 加密/解密开销**：Tailscale 基于 WireGuard，每个数据包都经过加解密处理。
5. **RTT 与同步等待**：虽然 ICMP ping 被禁无法测量，但 TCP 通信本身存在往返延迟，增加了每次 RPC 请求的同步开销。

### 4.2 首次传输 vs 缓存后

| 阶段 | 现象 |
| :--- | :--- |
| 首次 RPC 加载 | 需传输约 1.4 GB 张量，p001-r01 耗时约 120s，明显慢于后续 case |
| 启用 `--cache` 后 | 后续 case 缓存命中，模型加载时间降低，但每次启动仍有连接建立和缓存验证开销 |

### 4.3 实验结论

> 本实验通过 **Tailscale WireGuard VPN** 连接本地主机和 **USTC Vlab LXC**，成功验证了 llama.cpp RPC 双机推理功能。实验结果表明，在 Vlab 仅提供 2 核 CPU 的情况下，RPC 推理性能显著低于单机：prompt 处理速度降至约 12%，token 生成速度降至约 17%，质量测试平均耗时增加约 14 倍。这符合预期——RPC 的价值不在于提升单请求性能，而在于将计算任务分发到多台机器，实现更大规模的任务级并行。

---

## 5. 原始数据文件

| 文件 | 说明 |
| :--- | :--- |
| `lab4/data/results/rpc-single-bench-qwen35.jsonl` | llama-bench 单机组原始数据 |
| `lab4/data/results/rpc-distributed-bench-qwen35.jsonl` | llama-bench RPC 组原始数据 |
| `lab4/data/results/rpc-single-quality-qwen35.jsonl` | Rust 质量单机组原始数据 |
| `lab4/data/results/rpc-distributed-quality-qwen35.jsonl` | Rust 质量 RPC 组原始数据 |
