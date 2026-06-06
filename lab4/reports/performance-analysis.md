# Lab4 性能测试与系统分析

## 总览

本报告比较 `llama.cpp` 单机推理、参数优化和 RPC 分布式推理的性能表现。结论需要基于 JSONL 原始数据和可复现命令，不只写主观感受。

## 分项一：性能指标

| 指标 | 含义 | 系统原因 |
| :--- | :--- | :--- |
| 模型加载时间 | 从进程启动到模型可用 | 文件 I/O、mmap、页缓存、内存压力 |
| 首 Token 延迟 | 输入后生成第一个 token 的时间 | prefill、KV cache、调度、上下文长度 |
| 生成吞吐 | decode 阶段 tokens/s | CPU/GPU 算力、内存带宽、量化格式 |
| 总响应时间 | 端到端请求耗时 | 加载、prefill、decode、I/O |
| 内存占用 | 推理进程和缓存占用 | 模型大小、KV cache、batch、ctx |
| RPC 开销 | RPC 相比单机的额外耗时 | 网络、序列化、远端排队、同步等待 |

## 分项二：单机基线

汇总命令：

```bash
cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-summarize -- lab4/data/results/single-quality.jsonl
```

| 配置 | 平均耗时 ms | tokens/s | 备注 |
| :--- | ---: | ---: | :--- |
| baseline | 待填写 | 待填写 | 待填写 |

## 分项三：参数优化

| 参数组 | `--threads` | `--batch-size` | `--ctx-size` | `--n-gpu-layers` | 平均耗时 ms | tokens/s | 现象 |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | :--- |
| baseline | 待填写 | 待填写 | 待填写 | 待填写 | 待填写 | 待填写 | 待填写 |
| tuned-1 | 待填写 | 待填写 | 待填写 | 待填写 | 待填写 | 待填写 | 待填写 |

分析要点：

- 线程数是否接近物理核心数。
- batch 增大后吞吐和延迟是否同时变化。
- ctx 增大后内存占用是否明显上升。
- 是否观察到页缓存导致的重复测试差异。

## 分项四：输出质量

| Prompt | 类型 | 配置 A 评分 | 配置 B 评分 | 主要差异 |
| :--- | :--- | ---: | ---: | :--- |
| quality-zh-001 | 中文问答 | 待填写 | 待填写 | 待填写 |
| quality-summary-001 | 摘要 | 待填写 | 待填写 | 待填写 |
| quality-code-001 | 代码解释 | 待填写 | 待填写 | 待填写 |
| quality-reason-001 | 推理题 | 待填写 | 待填写 | 待填写 |
| quality-os-001 | OS 知识 | 待填写 | 待填写 | 待填写 |

## 分项五：RPC 对比

汇总命令：

```bash
cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-summarize -- lab4/data/results/rpc-quality.jsonl
```

| 模式 | 平均耗时 ms | tokens/s | 成功数 | 现象 |
| :--- | ---: | ---: | ---: | :--- |
| single | 待填写 | 待填写 | 待填写 | 待填写 |
| rpc | 待填写 | 待填写 | 待填写 | 待填写 |

分析要点：

- RPC 是否比单机更快；如果更慢，说明网络和同步开销。
- 主机和从机硬件是否异构。
- 网络是有线、无线还是热点。
- prompt 长度和生成长度是否足以摊薄 RPC 固定开销。
