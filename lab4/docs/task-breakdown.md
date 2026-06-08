# Lab4 任务拆解

## 总览

Lab4 要回答的核心问题是：如何在普通主机和小规模多机环境中部署、测量、优化并扩展
本地大模型推理系统。

本仓库采用：

- 主线：`llama.cpp` 本地与 RPC 推理，80 分。
- 扩展：Ray 批量推理任务调度，20 分。
- 加分：Ray 负载均衡、故障重试和 llama-server 并发压力测试。

正式推理由 `lab4/third_party/llama.cpp` submodule 提供；Rust 工具负责实验记录和
数据处理。Ray 官方接口部分使用 Python，并在报告中明确说明这一边界。

## 当前完成度

| 工作项 | 状态 | 证据 |
| :--- | :--- | :--- |
| 5 个以上性能指标与合理性 | 已完成 | `docs/param-optimization-report.md` |
| Qwen3.5-2B GGUF 本地部署 | 已完成 | `reports/smoke.md`、`docs/llama-cpp-setup.md` |
| 至少 3 个指标的实际测量 | 已完成 | Prompt/Generation 吞吐、启动耗时、端到端延迟 |
| 参数优化 | 已完成并重跑 | 线程、batch、输入长度、mmap |
| 5 类 Prompt 质量评估 | 已完成 | `data/prompts/quality-prompts.jsonl`、质量报告 |
| 双机 RPC | 已完成 | 本地主机 + USTC Vlab LXC |
| 单机与 RPC 对比 | 已完成 | `docs/rpc-experiment-report.md` |
| Ray 部署与 Task 调度 | 已完成 | 单机 head + 两类自定义资源 |
| 20 条批量 Prompt | 已完成 | 4 种模式，80 条请求记录 |
| Ray 性能对比与分析 | 已完成 | wall time、延迟、吞吐、节点统计 |
| Ray 负载均衡加分 | 已完成 | 30 条 Prompt、2 种真实 Ray 策略 |
| Ray 故障重试加分 | 已完成 | kill s2，失败 Task 重试到 s1，最终 100% |
| 并发压力测试 | 已完成 | 并发度 1、2、4 |
| 最终截图 | 待人工整理 | 本地推理、RPC、Ray status/日志 |

## 分项一：llama.cpp 主线

### 1. 指标设计

至少选取 5 个指标，并解释其系统意义：

- Prompt throughput：prefill 阶段矩阵计算和 batch 利用率。
- Generation throughput：逐 token decode、内存带宽和同步效率。
- 模型加载/启动时间：文件 I/O、`mmap`、缺页异常和页缓存。
- 端到端延迟：进程启动、模型加载、prefill、decode 和 RPC 的总成本。
- 内存占用：模型权重、KV cache、进程与 Ray runtime 的资源压力。
- CPU 利用率：线程数、P/E core 调度、上下文切换和缓存竞争。
- 成功率/P95：服务稳定性和长尾延迟。

### 2. 本地部署

固定信息：

| 项目 | 值 |
| :--- | :--- |
| 模型 | Qwen3.5-2B Q4_K_M |
| 文件 | `qwen3.5-2b-q4_k_m.gguf` |
| SHA-256 | `57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8` |
| llama.cpp commit | `c4a278d68efa17811006f2123a84081dac03fac7` |
| 后端 | CPU |

### 3. 参数优化

使用 `llama-bench` 比较：

- `--threads 4/8/12`
- `--batch-size 32/64/128`
- `--n-prompt 128/512/1024`
- 默认 `mmap` 与 `--mmap 0`

每条 benchmark 内部重复 3 次。报告同时说明笔记本 CPU 温度、后台负载和混合核心
调度造成的跨轮波动，避免把噪声误写成因果关系。

### 4. 输出质量

固定 5 条 Prompt，覆盖：

- 中文问答
- 摘要
- Rust 代码解释
- 系统推理
- OS 专项知识

比较不同温度配置的相关性、完整性、准确性、重复和幻觉。

### 5. 双机 RPC

主机运行 `llama-cli --rpc`，USTC Vlab LXC 运行 `rpc-server`。报告记录：

- 两端硬件和软件版本
- Tailscale 网络与端口
- 成功的设备发现和推理命令
- 单机/RPC Prompt 吞吐、Generation 吞吐、端到端耗时
- 2 vCPU 从机、TCP/VPN、同步和张量传输造成的性能下降

## 分项二：Ray 扩展

### 1. 部署

单机多进程模拟两台异构推理节点：

- s1：`llama-server --threads 8 --port 8080`
- s2：`llama-server --threads 4 --port 8081`
- Ray head 注册 `server_s1` 和 `server_s2` 自定义资源
- Ray Task 根据资源标签访问对应 HTTP 后端

由于两个后端共享同一 CPU，报告明确区分“调度功能验证”与“真实物理多机加速”。

### 2. 基础实验

固定前 20 条 Prompt，比较：

- serial
- fixed partition
- round robin
- latency based

记录请求开始/结束形成的端到端延迟、输出 token 数、总 wall time、吞吐和节点统计。
计时从 Ray Task 提交前开始，覆盖调度与结果回收。

### 3. 加分实验

- 负载均衡：同一 30 条 Prompt 比较 round robin 与 latency based。
- 故障重试：提交后终止 s2，把失败请求重新提交到 s1。
- 并发压力：并发度 1、2、4，记录平均/P95 延迟、吞吐和失败数。

## 分项三：OS 知识点

实验结论应联系以下机制：

- 进程、线程和 CPU 调度
- P/E core、上下文切换和线程过量
- 虚拟地址空间、`mmap`、缺页异常与页缓存
- KV cache 和内存工作集
- RPC 序列化、TCP、VPN、RTT 与同步等待
- Ray Task、资源标签、调度开销和故障恢复
- 并发下的吞吐/延迟权衡与长尾效应
- OOM 保护：Ray 在节点内存超过阈值时主动终止 worker

完整说明见 [OS 知识点](os-knowledge.md)。

## 提交检查

1. 确认 GGUF 权重仍被 `.gitignore` 排除。
2. 提交 Rust/Python 源码、Prompt、原始结果和汇总 JSON。
3. 核对报告引用的文件均存在。
4. 运行 Rust fmt、Clippy、测试和 Python unittest。
5. 补充必要截图并检查脱敏。
6. 在 **2026-06-08 23:59** 前提交。
