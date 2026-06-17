# Lab4 文档索引

## 总目标

Lab4 围绕 Qwen3.5-2B Q4_K_M 与 `llama.cpp`，完成本地部署、性能测量、参数优化、
输出质量评估、双机 RPC 和 Ray 批量任务调度。课程必做部分采用
“`llama.cpp` 80 分 + Ray 20 分”路线。

代码边界如下：

- Rust：Prompt 校验、命令包装、JSONL 记录、统计和存储测量。
- Python：仅用于 Ray 官方 Task API、HTTP 并发与参数实验编排。
- C/C++：不自行编写；`llama.cpp` 仅以 Git submodule 指针引入。

## 辅助文档

- [任务拆解](task-breakdown.md)：总分结构、评分点、完成状态和剩余材料。
- [OS 知识点](os-knowledge.md)：进程、线程、虚拟内存、页缓存、RPC 与调度分析。
- [Rust 实现约束](rust-implementation.md)：Rust 工具边界与编码规范。
- [llama.cpp 接入](llama-cpp-setup.md)：submodule、CMake 与 GGUF 模型准备。
- [RPC 双机手册](rpc-two-machine-setup.md)：主机与从机操作步骤。

## 正式实验报告

见 [`lab4/reports/`](../reports/) 目录：

- [Lab4 总报告](../reports/README.md)：评分点符合性、环境、命令、核心数据和系统分析汇总。
- [参数优化报告](../reports/param-optimization-report.md)：Qwen3.5 线程、batch、输入长度和 mmap。
- [输出质量报告](../reports/quality-evaluation-report.md)：Qwen3.5 两种温度配置的性能与人工评分。
- [RPC 实验报告](../reports/rpc-experiment-report.md)：单机与双机 RPC 对照。
- [Ray 实验报告](../reports/ray-experiment-report.md)：20 条 Prompt、四种执行模式。
- [Ray 加分报告](../reports/ray-bonus-report.md)：30 条负载均衡与 Ray 故障重试。
- [并发压力报告](../reports/concurrency-stress-report.md)：并发度 1、2、4 对照。

## 当前状态

已完成本地推理、参数优化、5 类质量 Prompt、双机 RPC、Ray 基础实验、30 条 Ray
负载均衡、Ray 故障重试和三档并发压力实验。模型和实验版本均已固定，原始数据位于
`lab4/data/results/`。

## 规范

- 不在本仓库维护 Makefile。
- GGUF 权重不提交 Git，只记录文件名、大小和 SHA-256。
- Rust 代码遵守 [Rust 编码规范](../../docs/src/team/conventions/rust.md)。
- 截止时间：**2026-06-08 23:59**。
