# Lab4 总报告：llama.cpp、RPC 与 Ray 推理实验

> 总报告仅用以快速了解本组 `lab4` 的进度，具体内容请于对应的报告进行细读。
>
> ## 实验文件对照表
>
> | 类别 | 任务 | 报告文件 | 关键脚本 / 数据 |
> | :--- | :--- | :--- | :--- |
> | **llama.cpp 主线** | 性能指标 + 参数优化 | [`param-optimization-report.md`](./param-optimization-report.md) | `scripts/param_optimization.py`, `data/results/param-opt/` |
> | **llama.cpp 主线** | 输出质量评估 | [`quality-evaluation-report.md`](./quality-evaluation-report.md) | `data/prompts/quality-prompts.jsonl`, `data/results/quality-*.jsonl` |
> | **llama.cpp 主线** | RPC 双机分布式推理 | [`rpc-experiment-report.md`](./rpc-experiment-report.md) | — |
> | **llama.cpp 选做** | `llama-server` 轻量并发请求测试 | [`concurrency-stress-report.md`](./concurrency-stress-report.md) | `scripts/concurrency_stress_test.py` |
> | **Ray 必做** | 批量任务调度 | [`ray-experiment-report.md`](./ray-experiment-report.md) | `scripts/ray_batch_inference.py`, `data/results/ray-batch-results.*` |
> | **Ray 选做** | 负载均衡调度（30 prompt） | [`ray-bonus-report.md`](./ray-bonus-report.md) | `data/results/ray-loadbalance-30/` |
> | **Ray 选做** | 失败重试（故障注入） | [`ray-bonus-report.md`](./ray-bonus-report.md) | `scripts/ray_failover_test.py`, `data/results/ray-failover/` |

## 1. 总体结论

本仓库 `lab4/` 当前选择 **llama.cpp 主线 + Ray 方向**，未选择
Ceph 方向。实验以 Qwen3.5-2B Q4_K_M GGUF 模型为对象，完成了本地推理、性能指标
设计、参数优化、质量测试、双机 RPC、Ray 批量调度，以及 Ray 负载均衡、失败重试和
并发压力测试。

报告、脚本、原始结果和截图
均已放在 `lab4/` 下；`reports/` 内本文件可作为快速审阅入口。

## 2. 课程要求符合性

### 2.1 llama.cpp 主线任务

| 要求 | 当前状态 | 证据摘要 |
| :--- | :--- | :--- |
| 不少于 5 个性能指标并说明合理性 | 已完成 | Prompt 吞吐、生成吞吐、模型加载时间、内存占用、CPU 利用率 |
| 选择 GGUF 模型并完成单机部署 | 已完成 | Qwen3.5-2B Q4_K_M，CPU 后端成功推理 |
| 至少 3 个指标实际测量 | 已完成 | `llama-bench`、Rust 批量工具、启动耗时和吞吐记录 |
| 参数分析、测试与优化 | 已完成 | 对比 `--threads`、`--batch-size`、`--n-prompt`、`mmap/no-mmap` |
| 5 个 prompt，至少 3 类质量评估 | 已完成 | 中文问答、摘要、代码解释、推理题、OS 相关问题共 5 类 |
| RPC 双机分布式推理 | 已完成 | 本地主机 + USTC Vlab LXC，通过 Tailscale 连接 |
| 单机与 RPC 性能对比 | 已完成 | 单机明显快于 Vlab RPC，并给出网络、CPU、缓存和同步原因 |

### 2.2 Ray 选择性必做任务

| 要求 | 当前状态 | 证据摘要 |
| :--- | :--- | :--- |
| Ray 环境部署与 head/worker 配置 | 已完成 | 单机 head，注册 `server_s1/server_s2` 自定义资源 |
| 至少两个推理节点或合理模拟 | 已完成 | 两个 `llama-server` 进程模拟异构节点，报告说明 Vlab 磁盘/内存限制 |
| 不少于 20 个 prompt | 已完成 | `batch-prompts.jsonl` 共 30 条，基础实验取前 20 条 |
| Ray Task/Actor 分发并收集结果 | 已完成 | 使用 Ray Task 调用两个 HTTP 推理后端，记录耗时、输出长度和 token 数 |
| 至少两种执行方式对比 | 已完成 | serial、fixed_partition、round_robin、latency_based 共 4 种 |
| 系统现象分析 | 已完成 | 分析 Ray 调度、模型复用、节点差异、CPU 竞争和请求粒度 |

说明：Ray 原始 JSONL 记录 `start_time_unix_ms`、`end_time_unix_ms`、`total_ms`、
`content_len`、`tokens_predicted` 等字段，覆盖任务要求的开始时间、结束时间、总耗时和输出长度。

### 2.3 Ray 加分项

| 加分项 | 当前状态 | 结果摘要 |
| :--- | :--- | :--- |
| 负载均衡调度 | 已完成 | 30 条 prompt，对比 round_robin 与 latency_based |
| 失败重试 | 已完成 | kill s2 后重试到 s1，最终 20/20 成功 |
| 并发压力测试 | 已完成 | 并发度 1、2、4，记录平均延迟、P95、吞吐和失败数 |
| 异构节点分析 | 部分覆盖 | 两个 server 用不同线程数模拟异构节点，但非真实两台硬件 |

## 3. 实验环境与版本

| 项目 | 内容 |
| :--- | :--- |
| 主机 | Arch Linux，Intel i7-13700H，约 15 GiB RAM，CPU 推理 |
| RPC 从机 | USTC Vlab LXC，Ubuntu，2 vCPU，约 6 GiB RAM |
| 网络 | Tailscale WireGuard VPN，RPC 端口 50052 |
| 模型 | `qwen3.5-2b-q4_k_m.gguf` |
| 模型大小 | 1,396,198,496 bytes |
| SHA-256 | `57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8` |
| llama.cpp | `c4a278d68efa17811006f2123a84081dac03fac7` |
| Ray 方式 | 单机 head + 两个 `llama-server` 进程模拟两节点 |

## 4. 关键复现命令

### 4.1 模型校验

```bash
sha256sum lab4/data/models/qwen3.5-2b-q4_k_m.gguf
```

期望输出：

```text
57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8  lab4/data/models/qwen3.5-2b-q4_k_m.gguf
```

### 4.2 本地推理冒烟

```bash
cd lab4/third_party/llama.cpp

./build/bin/llama-cli \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  -p "操作系统页缓存有什么作用？用中文回答。" \
  -n 128 -t 8 -c 1024 \
  --seed 42 --temp 0.2 \
  --reasoning off --reasoning-budget 0 \
  --no-display-prompt --simple-io --show-timings
```

### 4.3 参数优化

```bash
cd lab4
UV_CACHE_DIR=/tmp/dipecs-uv-cache uv run python scripts/param_optimization.py
```

### 4.4 RPC 从机与主机

Vlab 从机：

```bash
cd ~/lab4-rpc-worker/llama.cpp

./build-rpc-cpu/bin/rpc-server \
  --host 0.0.0.0 \
  --port 50052 \
  --threads 2 \
  --cache
```

本地主机：

```bash
cd lab4/third_party/llama.cpp

./build/bin/llama-cli \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --rpc <VLAB_TAILSCALE_IP>:50052 \
  --ctx-size 1024 --batch-size 64 \
  --threads 8 --n-predict 64 \
  --prompt "什么是 RPC" \
  --reasoning off --reasoning-budget 0 \
  --single-turn --no-display-prompt \
  --simple-io --show-timings
```

### 4.5 Ray 基础实验

启动两个 `llama-server`：

```bash
cd lab4/third_party/llama.cpp

./build/bin/llama-server \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --port 8080 -c 1024 --threads 8 -n 96 --cache-ram 0

./build/bin/llama-server \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --port 8081 -c 1024 --threads 4 -n 96 --cache-ram 0
```

启动 Ray：

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

运行调度脚本：

```bash
.venv/bin/python scripts/ray_batch_inference.py \
  --prompt-count 20 \
  --output data/results/ray-batch-results.jsonl
```

## 5. 核心结果

### 5.1 参数优化结果

| 分组 | Prompt (t/s) | Generation (t/s) | 结论 |
| :--- | ---: | ---: | :--- |
| threads=4 | 152.38 | 30.34 | baseline |
| threads=8 | 180.99 | **34.16** | decode 最优 |
| threads=12 | **208.73** | 33.89 | prefill 最优 |
| batch=32 | **203.56** | **31.00** | 本轮 batch 最优 |
| batch=64 | 152.80 | 29.37 | 工作集更大，吞吐下降 |
| batch=128 | 157.47 | 29.65 | 未继续提升 |
| n-prompt=128 | **154.06** | 29.64 | 短输入 prefill 更快 |
| n-prompt=512 | 147.26 | 30.89 | 输入变长吞吐下降 |
| n-prompt=1024 | 141.98 | **31.23** | decode 差异小，视为噪声 |
| mmap=on | **155.38** | 31.03 | 启动和常规运行推荐默认 mmap |
| no-mmap | 153.21 | **31.30** | 稳态吞吐接近 |

优化方案：CPU 交互式推理使用 `--threads 8~12`、`--batch-size 32`、默认 `mmap`，
并将 `--ctx-size` 控制在任务所需范围内，避免 KV cache 过大导致内存压力。偏交互式
生成时用 `--threads 8`，偏批量 prefill 时用 `--threads 12`。

### 5.2 单机 vs RPC

| 指标 | 单机 CPU | RPC (Vlab CPU) | RPC/单机 |
| :--- | ---: | ---: | ---: |
| Prompt 处理 | 213.48 t/s | 24.94 t/s | 0.12x |
| Token 生成 | 34.45 t/s | 5.80 t/s | 0.17x |
| 质量测试平均耗时 | 8,469.67 ms | 118,392.80 ms | 14.0x |
| 质量测试平均生成速度 | 31.84 t/s | 5.14 t/s | 0.16x |

RPC 性能下降符合预期：Vlab 只有 2 vCPU，且跨 Tailscale 传输张量、中间结果和同步消息；
首次 RPC 还需要传输约 1.4 GB 模型张量，后续依赖 `--cache` 降低重复传输成本。

### 5.3 Ray 基础调度

| 策略 | 总耗时 (s) | 吞吐 (prompts/s) | 吞吐 (tokens/s) | 平均延迟 (ms) | 成功率 |
| :--- | ---: | ---: | ---: | ---: | ---: |
| serial | 65.21 | 0.307 | 29.44 | 3260 | 20/20 |
| fixed_partition | 61.89 | 0.323 | 31.02 | 5554 | 20/20 |
| **round_robin** | **61.26** | **0.327** | **31.34** | 5552 | 20/20 |
| latency_based | 65.92 | 0.303 | 29.12 | 5603 | 20/20 |

并行策略减少总耗时，但单请求延迟上升；原因是两个 `llama-server` 共享同一 CPU，任务级并行
提升了批量吞吐，同时引入 CPU 时间片、缓存和内存带宽竞争。

### 5.4 Ray 加分实验

负载均衡 30 条 prompt：

| 策略 | 总耗时 (s) | 吞吐 (prompts/s) | 平均延迟 (ms) | s1 请求数 | s2 请求数 |
| :--- | ---: | ---: | ---: | ---: | ---: |
| round_robin | **93.77** | **0.320** | **5564.1** | 15 | 15 |
| latency_based | 98.56 | 0.304 | 5697.4 | 16 | 14 |

失败重试：

| 阶段 | 成功 | 失败 | 说明 |
| :--- | ---: | ---: | :--- |
| Phase 1 | 10 | 10 | s2 被 kill 后，分配给 s2 的请求失败 |
| Phase 2 | 10 | 0 | 失败请求重试到 s1 |
| 最终 | **20** | **0** | 最终成功率 100% |

并发压力：

| 并发度 | 总耗时 (s) | 吞吐 (prompts/s) | 平均延迟 (ms) | P95 延迟 (ms) | 失败数 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 65.97 | 0.30 | 3298.6 | 3416.4 | 0 |
| 2 | 46.27 | 0.43 | 4624.9 | 4850.5 | 0 |
| 4 | **32.40** | **0.62** | 6467.3 | 6608.1 | 0 |

## 6. 输出质量评估

质量 prompt 共 5 条，覆盖 5 个类别：

| 类别 | Prompt 目标 |
| :--- | :--- |
| 中文问答 | 解释本地推理与云端推理优缺点 |
| 摘要 | 将 OS、虚拟内存、文件系统、网络栈相关段落概括为三点 |
| 代码解释 | 解释 Rust `read_config` 为什么返回 `Result` |
| 推理题 | 分析 RPC 比单机慢的系统原因 |
| OS 课程问题 | 解释 `mmap`、页缓存和缺页异常对 GGUF 加载时间的影响 |

固定其他参数，比较三种 `temperature`：

| 配置 | 成功数 | 平均端到端耗时 | 平均生成速度 | 人工评分 |
| :--- | ---: | ---: | ---: | ---: |
| temperature=0.2 | 5/5 | 8,868.8 ms | 31.58 t/s | 13/20 |
| **temperature=0.5** | 5/5 | **8,436.4 ms** | **32.12 t/s** | **14/20** |
| temperature=0.8 | 5/5 | 9,454.2 ms | 28.90 t/s | 13/20 |

**temperature=0.5 综合质量最优**，避免了 0.2 的"异步"误述和 0.8 的"零页映射"幻觉。
部署层面配置（如 threads）经验证不影响输出质量，仅改变推理速度。

## 7. OS 与分布式系统知识点总结

| 知识点 | 在本实验中的体现 |
| :--- | :--- |
| 进程与线程调度 | `--threads` 改变 CPU 并行度；P-core/E-core 与上下文切换影响吞吐 |
| 虚拟内存与 `mmap` | GGUF 通过内存映射按需加载，页缓存降低重复启动开销 |
| 缺页异常与页缓存 | 首次访问模型页会触发缺页，warm-cache 后加载更快 |
| KV cache | `--ctx-size` 越大，KV cache 内存越高；Vlab 上曾因默认上下文过大触发 OOM |
| Batch 与缓存局部性 | 增大 batch 不总是更快，工作集超过缓存后可能降低吞吐 |
| RPC 与网络协议栈 | 张量传输、WireGuard 加密、RTT 和同步等待共同增加 RPC 开销 |
| Ray 任务调度 | Ray 提升批量任务吞吐，但短任务会受到调度和 warm-up 成本影响 |
| 故障恢复 | 通过失败检测和重试把失败请求转发到健康节点，提高最终成功率 |

## 8. 文件与证据清单

| 类型 | 路径 |
| :--- | :--- |
| 总报告 | `lab4/reports/README.md` |
| 参数优化报告 | `lab4/reports/param-optimization-report.md` |
| 输出质量报告 | `lab4/reports/quality-evaluation-report.md` |
| RPC 双机报告 | `lab4/reports/rpc-experiment-report.md` |
| Ray 基础报告 | `lab4/reports/ray-experiment-report.md` |
| Ray 加分报告 | `lab4/reports/ray-bonus-report.md` |
| 并发压力报告 | `lab4/reports/concurrency-stress-report.md` |
| 原始结果 | `lab4/data/results/` |
| Prompt 数据 | `lab4/data/prompts/quality-prompts.jsonl`、`lab4/data/prompts/batch-prompts.jsonl` |
| 本地推理截图 | `lab4/assets/01-local-inference.png` |
| RPC 截图 | `lab4/assets/02-rpc-device-discovery.png`、`lab4/assets/03-rpc-inference.png` |
| Ray 截图 | `lab4/assets/04-ray-status.png`、`lab4/assets/05-ray-experiment-results.png` |
| 实验脚本 | `lab4/scripts/`、`lab4/crates/lab4-tools/` |

## 9. 提交注意事项

- GGUF 模型权重体积较大，不应提交到 Git；提交报告中保留模型名、大小和 SHA-256。
- Ceph 未选用，旧 Ceph 分析文件已放在 `reports/archive/`，不作为当前 20 分扩展路线。
- Ray 采用单机多进程模拟，不是真实 Ray 双机集群；报告已说明 Vlab 资源限制和模拟合理性。
