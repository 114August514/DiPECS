# Lab4 Ray 批量推理任务调度实验报告

## 1. 实验环境

### 1.1 部署说明

由于 Vlab LXC 磁盘仅剩 3.5G，不足以额外存放一份 1.3G GGUF 模型供 `llama-server` 加载，本次 Ray 调度实验采用**单机多进程模拟多机环境**：在同一台主机上部署两个 `llama-server` 实例和两个 Ray worker，模拟两台异构机器组成的推理集群。

| 组件 | 配置 |
| :--- | :--- |
| 主机 | Linux archlinux, i7-13700H, 15GiB RAM |
| Ray runtime | 单机 head，注册 `server_s1`、`server_s2` 两类自定义资源 |
| Ray Task 1 | 请求 `server_s1: 0.6`，对应 llama-server@8080 |
| Ray Task 2 | 请求 `server_s2: 0.6`，对应 llama-server@8081 |
| Server s1（模拟较快机器） | `--threads 8 --port 8080` |
| Server s2（模拟较慢机器） | `--threads 4 --port 8081` |
| 模型 | qwen3.5-2b-q4_k_m.gguf (Q4_K_M, ~1.3GB) |

### 1.2 启动命令

**llama-server（两个实例）：**

```bash
cd lab4/third_party/llama.cpp

# Server s1：模拟较快节点（threads=8）
./build/bin/llama-server \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --port 8080 -c 1024 --threads 8 -n 96 --cache-ram 0

# Server s2：模拟较慢节点（threads=4）
./build/bin/llama-server \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --port 8081 -c 1024 --threads 4 -n 96 --cache-ram 0
```

**Ray cluster：**

```bash
ray start --head \
  --port=6379 \
  --num-cpus=2 \
  --resources='{"server_s1": 1, "server_s2": 1}' \
  --object-store-memory=134217728 \
  --include-dashboard=false \
  --disable-usage-stats
```

**Ray 脚本：**

```bash
.venv/bin/python scripts/ray_batch_inference.py \
  --prompt-count 20 \
  --output data/results/ray-batch-results.jsonl
```

这里直接使用 `.venv/bin/python`，而不是 `uv run`。当前 Ray 版本会在 `uv run` 环境中
自动构造 runtime environment 并分发 working directory，单机实验会产生无必要的
目录复制和依赖安装，曾在内存达到 95.2% 时触发 Ray OOM worker kill。

---

## 2. 实验设计

### 2.1 Prompt 数据集

使用 `lab4/data/prompts/batch-prompts.jsonl`，共 **20 个 prompt**，覆盖操作系统、RPC、Ceph 三类问题，每个 prompt 的 `max_tokens=96`。

| 类别 | 数量 | 示例 |
| :--- | ---: | :--- |
| OS | 10 | "解释进程和线程在 CPU 推理任务中的区别" |
| RPC | 5 | "RPC 分布式推理会引入哪些网络开销？" |
| Ceph | 5 | "Ceph 中 Monitor 和 OSD 分别负责什么？" |

### 2.2 调度策略

测试了 4 种执行方式：

| 策略 | 说明 |
| :--- | :--- |
| **serial** | 串行调用 server s1，作为基准对照 |
| **fixed_partition** | 固定分区：前 10 个给 s1，后 10 个给 s2 |
| **round_robin** | 轮询：s1 → s2 → s1 → s2 ... |
| **latency_based** | 基于历史延迟：warm-up 测试后，按延迟反比例分配权重 |

---

## 3. 实验结果

### 3.1 总体性能

| 策略 | 总耗时 (s) | 吞吐 (prompts/s) | 吞吐 (tokens/s) | 平均延迟 (ms) | 成功率 |
| :--- | ---: | ---: | ---: | ---: | ---: |
| serial | 67.08 | 0.298 | 28.62 | 3354 | 20/20 |
| fixed_partition | 64.00 | 0.313 | 30.00 | 5529 | 20/20 |
| **round_robin** | **62.39** | **0.321** | **30.78** | 5595 | 20/20 |
| latency_based | 67.76 | 0.295 | 28.33 | 5692 | 20/20 |

### 3.2 各节点负载分布

**fixed_partition：**

| 节点 | 请求数 | 平均延迟 (ms) | P95 延迟 (ms) |
| :--- | ---: | ---: | ---: |
| s1 (threads=8) | 10 | 4808 | 4930 |
| s2 (threads=4) | 10 | 6250 | 7837 |

**round_robin：**

| 节点 | 请求数 | 平均延迟 (ms) | P95 延迟 (ms) |
| :--- | ---: | ---: | ---: |
| s1 (threads=8) | 10 | 4954 | 5223 |
| s2 (threads=4) | 10 | 6235 | 7610 |

**latency_based：**

| 节点 | 请求数 | 平均延迟 (ms) | P95 延迟 (ms) |
| :--- | ---: | ---: | ---: |
| s1 (threads=8) | 11 | 4874 | 5084 |
| s2 (threads=4) | 9 | 6692 | 7957 |

---

## 4. 结果分析

### 4.1 为什么并行减少了总耗时但增加了单请求延迟

| 现象 | 原因 |
| :--- | :--- |
| 串行延迟最低 (3240ms) | 无并发竞争，CPU 独享 |
| 并行平均延迟上升 (~5400ms) | 两个 llama-server 共享同一物理 CPU，并发导致线程竞争和缓存失效 |
| 并行总耗时仍低于串行 | 两个请求同时处理，重叠了 I/O 等待和计算时间 |

### 4.2 不同调度策略的对比

| 策略 | 优势 | 劣势 |
| :--- | :--- | :--- |
| fixed_partition | 实现简单，无调度开销 | 未考虑节点性能差异，s2 拖慢整体 |
| round_robin | 负载均衡，公平性最好 | 同样未考虑性能差异 |
| latency_based | 给快节点更多请求，适合长期服务复用历史数据 | 本轮把两次 warm-up 纳入 wall time，短批次反而更慢 |

本轮 **round_robin 比 serial 总耗时低约 7.0%**，是四种模式中吞吐最高的策略。
`latency_based` 的两次 warm-up 也计入总 wall time，因此在只有 20 条请求的短批次中
未能回收探测成本。若服务长期运行，应缓存节点历史延迟，而不是每个批次重新 warm-up。

### 4.3 节点性能差异

s1（threads=8）的平均延迟比 s2（threads=4）低约 **20-25%**，验证了线程数对推理速度的影响。在 latency_based 策略中，s1 分配到 11 个请求，s2 分配到 9 个，权重接近理论计算的 `s2_latency / (s1_latency + s2_latency) ≈ 0.55`。

---

## 5. 实验结论

> 本实验通过 **单机多进程模拟** 的方式，验证了 Ray 在批量推理任务调度中的核心价值。实验结果表明：
>
> 1. **任务级并行** 可以减少批量请求的总 wall time，即使单请求延迟因 CPU 竞争而上升。
> 2. **调度开销必须计入测量边界**：本轮计时从 Ray Task 提交前开始，包含调度和结果回收。
> 3. 延迟感知策略不是天然更快；短批次中 warm-up 成本可能超过负载分配收益。

---

## 6. 原始数据

| 文件 | 说明 |
| :--- | :--- |
| `lab4/data/results/ray-batch-results.jsonl` | 每个请求的详细结果（20 prompt × 4 策略 = 80 条） |
| `lab4/data/results/ray-batch-results.summary.json` | 四种策略的汇总统计 |
| `lab4/scripts/ray_batch_inference.py` | 实验脚本源码 |
