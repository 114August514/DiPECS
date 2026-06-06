# Lab4 文档索引

本目录先完成 Lab4 的任务梳理与知识点整理，后续代码和实验记录均围绕这里定义的路线展开。

参考资料来自 `/home/august/Work/os/osh-2026.github.io/docs/lab4`：

- `README.md`：Lab4 总要求、评分和提交说明。
- `llama_cpp.md`：llama.cpp 单机部署、性能参数、RPC 分布式推理和评估方式。
- `ray.md`：使用 Ray 做多机批量推理任务调度。
- `ceph.md`：使用 Ceph 做模型、prompt 和日志的分布式存储。

## 总目标

Lab4 要围绕本地大模型推理系统完成一次从部署、测量、优化到分布式扩展的系统实验。主线任务对应课程中的 `llama.cpp` 部分；本仓库使用 `llama.cpp` 完成正式推理，用 Rust 编写实验工具和数据处理代码。扩展任务需要在 Ray 和 Ceph 中二选一。

结合本仓库“代码必须使用 Rust 编写、不能出现 Makefile 文件”的约束，默认建议选择 **Ceph 方向** 作为扩展任务：Ceph 的模型文件、prompt 数据和日志读写可以自然地由 Rust 测试工具封装；Ray 官方实验路径偏 Python，若坚持 Rust-only，会增加不必要的适配成本。

## 分项文档

- [任务拆解](task-breakdown.md)：按“总-分”结构列出必做、选做、交付物和建议执行顺序。
- [OS 知识点](os-knowledge.md)：总结本实验涉及的操作系统与分布式系统知识点。
- [Rust 实现约束](rust-implementation.md)：约定后续代码目录、工具边界、测试方式和仓库规范。
- [llama.cpp 与 GGUF 模型接入](llama-cpp-setup.md)：说明 submodule、CMake 构建和模型下载流程。

## 当前执行路线

1. 先完成文档与实验设计。
2. 使用 `lab4/crates/lab4-tools` 中的 Rust 工具记录 prompt、命令、存储和汇总数据。
3. 使用 `llama.cpp` 完成正式单机推理、性能测量、参数优化和 RPC 分布式推理。
4. 扩展任务优先选择 Ceph：完成共享数据路径、存储指标测试和本地路径对比。
5. 汇总部署说明、性能分析、系统原因解释和必要截图。

## 交付边界

- 文档、配置、数据样例和 Rust 工具放在 `lab4/` 下。
- 不创建 `Makefile`，所有自动化入口使用 `cargo`、Rust 二进制或普通文档命令说明。
- Rust 代码遵守 [Rust 编码规范](../../docs/src/team/conventions/rust.md)。
- 参考文档中的截止时间为 **2026-06-08 23:59**，实验记录应尽早固定环境和命令。

## 已落地内容

- `lab4/crates/lab4-tools`：实验测量、命令封装、smoke、RPC 辅助和数据整理工具。
- `lab4/third_party/`：第三方依赖指针目录，推荐用 submodule 放 `llama.cpp`。
- `lab4/data/models/placeholder.model`：用于验证 Rust llama 流程的占位模型，不作为正式实验模型。
- `lab4/data/prompts/quality-prompts.jsonl`：5 条输出质量评估 prompt。
- `lab4/data/prompts/batch-prompts.jsonl`：20 条批量测试 prompt。
- `lab4/reports/deployment.md`：部署说明模板。
- `lab4/reports/performance-analysis.md`：性能分析模板。
- `lab4/reports/ceph-analysis.md`：Ceph 扩展分析模板。
- `lab4/reports/smoke.md`：真实 GGUF 单机推理与基准冒烟结果。
