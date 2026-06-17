# Lab4 Ray 选做加分实验报告

## 选做一：负载均衡调度（5 分）

### 1.1 实验设计

使用 **30 个 prompt**（`batch-prompts.jsonl`，追加 10 个自定义 prompt），测试两种调度策略：

| 策略 | 说明 |
| :--- | :--- |
| **round_robin** | 轮询：s1 → s2 → s1 → s2 ... |
| **latency_based** | 基于历史延迟：warm-up 测试后，按延迟反比例分配权重 |

### 1.2 实验环境

| 项目 | 内容 |
| :--- | :--- |
| 主机 | Linux archlinux, i7-13700H |
| Server s1（模拟较快节点） | `--threads 8 --ctx-size 1024 --port 8080` |
| Server s2（内存受限节点） | `--threads 4 --ctx-size 512 --parallel 1 --port 8081` |
| 模型 | qwen3.5-2b-q4_k_m.gguf |
| Ray 运行方式 | 单机 head + `server_s1/server_s2` 自定义资源 |
| 实现方式 | Ray Task 调用两个 llama-server 后端 |

s2 使用单 slot 和较小上下文，以便在 16 GiB 主机上同时容纳两个模型服务与 Ray。
本实验 Prompt 很短且最多生成 96 token，不会触及 512 token 上下文上限。

### 1.3 运行命令

**启动两个 llama-server：**

```bash
cd lab4/third_party/llama.cpp

# Server s1（模拟较快节点）
./build/bin/llama-server \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --port 8080 -c 1024 --threads 8 -n 96 --cache-ram 0

# Server s2（内存受限节点）
./build/bin/llama-server \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --port 8081 -c 512 --threads 4 --parallel 1 -n 96 --cache-ram 0
```

**启动 Ray：**

```bash
cd lab4

.venv/bin/ray start --head \
  --port=6379 \
  --num-cpus=2 \
  --resources='{"server_s1": 1, "server_s2": 1}' \
  --object-store-memory=134217728 \
  --include-dashboard=false \
  --disable-usage-stats
```

**运行负载均衡调度实验：**

```bash
cd lab4

# 仅测试 round_robin 和 latency_based 两种策略，取 30 个 prompt
.venv/bin/python scripts/ray_batch_inference.py \
  --prompt-count 30 \
  --strategies round_robin latency_based \
  --output data/results/ray-loadbalance-30/ray-loadbalance-30.jsonl
```

### 1.4 实验结果

| 策略 | 总耗时 (s) | 吞吐 (p/s) | 平均延迟 (ms) | s1 请求数 | s2 请求数 |
| :--- | ---: | ---: | ---: | ---: | ---: |
| round_robin | **93.77** | **0.320** | **5564.1** | 15 | 15 |
| latency_based | 98.56 | 0.304 | 5697.4 | 16 | 14 |

**各节点延迟详情**：

| 策略 | 节点 | 请求数 | 平均延迟 (ms) | P95 延迟 (ms) |
| :--- | :--- | ---: | ---: | ---: |
| round_robin | s1 (threads=8) | 15 | 4881.7 | 5444.3 |
| round_robin | s2 (threads=4) | 15 | 6246.5 | 7831.7 |
| latency_based | s1 (threads=8) | 16 | 4989.2 | 5179.1 |
| latency_based | s2 (threads=4) | 14 | 6506.8 | 7775.7 |

### 1.5 结果分析

1. **轮询调度**的总耗时优于延迟调度（93.77s vs 98.56s）。延迟策略的 wall time
   包含两个后端各一次 warm-up，短批次中探测成本未能被后续调度收益抵消。
2. **延迟调度**给 s1 分配了 16 个请求（53.3%），s2 分配了 14 个（46.7%），接近理论权重 `s2_latency / (s1_latency + s2_latency) = 3702.6 / (3123.9 + 3702.6) ≈ 0.54`。
3. s1 的平均延迟持续低于 s2，但两个 server 共享同一物理 CPU，仍会发生 CPU、
   缓存和内存带宽竞争。真实多机环境能消除这部分共享资源竞争。

---

## 选做二：失败重试（5 分）

### 2.1 实验设计

1. 并发提交 20 个 prompt（轮询分配到 s1/s2）
2. 提交后立即 **kill server s2（port 8081）** 注入故障
3. 收集 Phase 1 结果，s2 的 10 个请求全部失败
4. 将失败的 10 个请求重试到 s1
5. 记录重试日志和最终成功率

### 2.2 故障注入方式

```bash
pkill -f "llama-server.*port 8081"
```

在 prompt 提交后立即执行，使得已分配给 s2 的并发请求在连接时遭遇 `Connection refused` 或超时。

### 2.3 运行命令

**启动两个 llama-server（与负载均衡实验相同）：**

```bash
cd lab4/third_party/llama.cpp

# Server s1
./build/bin/llama-server \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --port 8080 -c 1024 --threads 8 -n 96 --cache-ram 0

# Server s2
./build/bin/llama-server \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --port 8081 -c 512 --threads 4 --parallel 1 -n 96 --cache-ram 0
```

**启动 Ray（已在运行时可跳过）：**

```bash
cd lab4

.venv/bin/ray start --head \
  --port=6379 \
  --num-cpus=2 \
  --resources='{"server_s1": 1, "server_s2": 1}' \
  --object-store-memory=134217728 \
  --include-dashboard=false \
  --disable-usage-stats
```

**运行失败重试实验：**

```bash
cd lab4

.venv/bin/python scripts/ray_failover_test.py
```

脚本内部会在提交请求后自动执行 `pkill` 注入故障，无需手动干预。

### 2.4 实验结果

| 阶段 | 成功 | 失败 | 说明 |
| :--- | ---: | ---: | :--- |
| Phase 1（初始请求） | 10 | 10 | s1 的 10 个请求成功，s2 的 10 个请求因 server 被 kill 而失败 |
| Phase 2（重试到 s1） | 10 | 0 | 全部 10 个失败请求重试到 s1 后成功 |
| **最终** | **20** | **0** | **成功率 100%** |

### 2.5 重试日志示例

```text
[Phase 1] Success: 10/20, Failed: 10
  - FAILED: batch-os-001 (assigned to s2): Connection refused
  - FAILED: batch-os-003 (assigned to s2): Connection refused
  ... (共 10 个)

[Phase 2] Retrying 10 failed prompts on server s1...
  - RETRY SUCCESS: batch-os-001 on s1, latency: 3120ms
  - RETRY SUCCESS: batch-os-003 on s1, latency: 3150ms
  ... (共 10 个)
```

### 2.6 结论

> 故障注入实验验证了推理服务的容错能力。在 server s2 被强制终止后，10 个原本分配给 s2 的请求全部失败（表现为 HTTP 连接超时或拒绝连接）。通过将失败请求重试到健康的 s1 节点，最终成功率达到 **100%**。这说明在生产环境中，配合健康检查和自动重试机制，可以有效应对单节点故障，保障推理服务的高可用性。

---

## 原始数据

| 文件 | 说明 |
| :--- | :--- |
| `lab4/data/results/ray-loadbalance-30/ray-loadbalance-30-detail.jsonl` | Ray 负载均衡详细结果（60 条） |
| `lab4/data/results/ray-loadbalance-30/ray-loadbalance-30-detail.summary.json` | Ray 负载均衡汇总 |
| `lab4/data/results/ray-failover/ray-failover-detail.jsonl` | Ray 故障注入与重试详细结果 |
| `lab4/data/results/ray-failover/ray-failover-summary.json` | Ray 故障重试汇总 |
| `lab4/scripts/ray_batch_inference.py` | Ray 基础和 30 条负载均衡脚本 |
| `lab4/scripts/ray_failover_test.py` | Ray 故障注入与重试脚本 |
