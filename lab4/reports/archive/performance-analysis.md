# Lab4 性能测试与系统分析

## 总览

本报告记录 Qwen2.5-1.5B-Instruct Q4_K_M 在 `llama.cpp` CPU 后端上的单机测试。
测试机器为 Intel Core i7-13700H，14 个物理核心、20 个逻辑核心，内存约 15.2 GiB。
除特别说明外，模型、随机种子和测试期间的软件版本保持不变。

当前已完成单机冒烟、线程数、batch、上下文、`mmap`、温度对比和双机 RPC。
扩展任务选择 Ray，Ceph 不属于当前交付路线。

## 分项一：性能指标

| 指标 | 测量方式 | 系统含义 |
| :--- | :--- | :--- |
| 进程端到端耗时 | Rust 包装器记录子进程墙钟时间 | 包含进程启动、模型加载、prefill 和 decode |
| 启动短请求耗时 | 单 token、`--no-warmup`，重复 5 次 | 近似观察模型映射和初始化开销，不等同于纯加载时间 |
| Prompt throughput | `llama-bench` 的 `pp` token/s | 反映 prefill、矩阵计算和 batch 利用率 |
| Generation throughput | `llama-bench` 的 `tg` token/s | 反映逐 token decode、内存带宽和线程调度 |
| 稳定性 | 3 至 5 次重复的标准差、最小值和最大值 | 反映调度、缓存和后台负载造成的抖动 |
| 输出质量 | 固定 5 条 prompt 的人工评分 | 防止只优化吞吐而忽略准确性和指令遵循 |
| RPC 开销 | 单机与双机 RPC 的同任务差值 | 已完成，见 RPC 报告 |

`llama-cli` 当前输出不能可靠分离首 token 时间，因此本报告不把端到端耗时误写为
TTFT。后续常驻 `llama-server` 测试可补充真实 TTFT 和并发延迟。

## 分项二：单机基线

`llama-cli` 五条 prompt 冒烟结果：

| 配置 | 成功数 | 平均端到端耗时 | 平均生成吞吐 |
| :--- | ---: | ---: | ---: |
| `threads=8, ctx=1024, batch=512` | 5/5 | 4447.40 ms | 42.64 token/s |

`llama-bench -t 8 -p 128 -n 64 -r 3` 结果：

| 阶段 | 平均吞吐 | 标准差 |
| :--- | ---: | ---: |
| Prompt processing | 240.97 token/s | 2.48 |
| Token generation | 46.22 token/s | 0.44 |

原始数据：

- `lab4/data/results/smoke-llama-cpp-quality.jsonl`
- `lab4/data/results/smoke-llama-bench.jsonl`

## 分项三：线程数

固定 `p=128`、`n=64`，每组重复 3 次：

| Threads | Prompt token/s | Prompt 标准差 | Generation token/s | Generation 标准差 |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 55.73 | 0.19 | 17.28 | 0.05 |
| 2 | 109.66 | 0.69 | 29.68 | 0.20 |
| 4 | 201.59 | 5.61 | 43.25 | 0.41 |
| 8 | 244.84 | 0.92 | 45.56 | 0.30 |
| 12 | **306.49** | 0.28 | **46.06** | 0.72 |
| 14 | 303.42 | 0.78 | 43.71 | 0.22 |
| 20 | 166.79 | 38.97 | 21.28 | 5.48 |

12 线程是本机当前最佳点。14 线程开始出现 decode 回退，20 线程的吞吐和稳定性都
显著恶化。i7-13700H 是混合架构处理器，逻辑核心并不等价；线程过多会增加调度、
缓存竞争和同步开销，不能按逻辑核心数线性扩展。

原始数据：`lab4/data/results/threads-matrix.jsonl`。

## 分项四：Batch 大小

固定 `threads=12`、`p=512`、`n=64`，每组重复 3 次：

| Batch | Prompt token/s | Prompt 标准差 | Generation token/s | Generation 标准差 |
| ---: | ---: | ---: | ---: | ---: |
| 64 | **271.71** | 1.96 | **46.83** | 0.47 |
| 128 | 211.40 | 0.68 | 43.36 | 0.22 |
| 256 | 218.95 | 1.66 | 42.96 | 0.40 |
| 512 | 220.29 | 0.23 | 42.63 | 0.21 |
| 1024 | 219.72 | 0.50 | 42.91 | 0.43 |
| 2048 | 215.04 | 4.92 | 42.15 | 0.88 |

本机 CPU 后端在该模型和输入长度下以 batch 64 最快。继续增大 batch 没有提高
矩阵计算利用率，反而增加工作集和缓存压力。batch 2048 的标准差也明显增大。

原始数据：`lab4/data/results/batch-matrix.jsonl`。

## 分项五：mmap 与页缓存

启动短请求固定 `threads=12`、`ctx=512`、`batch=64`、生成 1 token，并关闭 warmup：

| 模式 | 重复次数 | 平均耗时 | 最小值 | 最大值 |
| :--- | ---: | ---: | ---: | ---: |
| `mmap` | 5 | **676.2 ms** | 663 ms | 697 ms |
| `no-mmap` | 5 | 904.0 ms | 876 ms | 949 ms |

在页缓存已经较热的当前环境中，`mmap` 的短请求启动时间比 `no-mmap` 低约 25.2%。
原因是 `mmap` 先建立虚拟地址映射，文件页按需进入物理内存；`no-mmap` 更偏向在
初始化阶段显式读取权重。

固定 `threads=12`、`batch=64`、`p=128`、`n=64` 的 `llama-bench` 结果：

| 模式 | Prompt token/s | Prompt 标准差 | Generation token/s | Generation 标准差 |
| :--- | ---: | ---: | ---: | ---: |
| `no-mmap` | **289.25** | 14.84 | **46.37** | 1.69 |
| `mmap` | 208.74 | 2.75 | 43.26 | 0.24 |

这两组结果并不矛盾：`mmap` 改善了进程启动，而 `no-mmap` 在权重已读入后取得更高
的计算阶段吞吐，但抖动也更大。由于无法在普通用户权限下可靠清空系统页缓存，本组
数据属于 warm-cache 结果，不代表冷启动磁盘性能。

原始数据：

- `lab4/data/results/startup-mmap.jsonl`
- `lab4/data/results/startup-no-mmap.jsonl`
- `lab4/data/results/mmap-matrix.jsonl`

## 分项六：上下文大小

短 prompt、生成 1 token、`mmap`、`--no-warmup`，每组重复 3 次：

| Context | 平均端到端耗时 | 最小值 | 最大值 |
| ---: | ---: | ---: | ---: |
| 512 | **682.3 ms** | 676 ms | 693 ms |
| 1024 | 690.0 ms | 681 ms | 695 ms |
| 2048 | 741.7 ms | 726 ms | 773 ms |
| 4096 | 728.0 ms | 720 ms | 736 ms |

短输入下 512 与 1024 差异很小，增大到 2048 或 4096 后初始化成本略升。结果没有
严格单调，说明几十毫秒级差异仍会受到缓存和调度抖动影响。更大的上下文会扩大
KV cache 容量，但本轮没有采集进程峰值 RSS，因此不对具体内存增量作无依据推断。

原始数据：`lab4/data/results/ctx-*.jsonl`。

## 分项七：输出质量

两组配置固定 `threads=12`、`ctx=1024`、`batch=64`、`seed=42`，仅比较
`temperature=0.2` 与 `temperature=0.8`：

| 配置 | 成功数 | 平均端到端耗时 | 平均生成吞吐 | 人工评分均值 |
| :--- | ---: | ---: | ---: | ---: |
| temperature 0.2 | 5/5 | 5773.6 ms | **43.74 token/s** | 11.8 / 20 |
| temperature 0.8 | 5/5 | **5486.0 ms** | 41.32 token/s | **13.8 / 20** |

单次生成的长度不同，因此不能把平均耗时差异直接归因于温度。高温组在代码解释和
RPC 原因题上更完整，但两组都未按要求把摘要写成三点，并且 OS 题均错误展开 GGUF
缩写、过度简化 `mmap`。详细评分与错误说明见
`lab4/reports/quality-evaluation.md`。

## 分项八：推荐配置

当前 CPU 交互式推理推荐：

```text
threads=12, batch=64, ctx=1024, mmap=on
```

选择依据是线程与 batch 吞吐最高、1024 上下文的短请求开销接近 512，并且 `mmap`
能降低每次独立进程加载模型时的启动耗时。如果改成常驻服务且只追求 warm-state
吞吐，可重新测试 `no-mmap`，不能直接沿用本轮启动型工作负载的结论。

GPU offload 未测试。机器虽然有 NVIDIA RTX 4060 Laptop GPU，但本次 `llama.cpp`
构建和所有数据均为 CPU 后端，因此 `--n-gpu-layers` 标记为不适用。

## 分项九：RPC 对比

固定 Qwen3.5-2B Q4_K_M、Prompt 和生成长度后的结果：

| 指标 | 单机 CPU | RPC（Vlab 2 vCPU） |
| :--- | ---: | ---: |
| Prompt throughput | 213.48 t/s | 24.94 t/s |
| Generation throughput | 34.45 t/s | 5.80 t/s |
| 质量组平均端到端耗时 | 8,469.67 ms | 118,392.80 ms |
| 成功率 | 15/15 | 15/15 |

RPC 明显慢于单机，主要原因是从机只有 2 vCPU，同时引入了张量传输、TCP/VPN、
远端排队和同步等待。完整命令与分析见
[`lab4/reports/rpc-experiment-report.md`](../rpc-experiment-report.md)。
