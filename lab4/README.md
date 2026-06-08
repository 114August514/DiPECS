# OS Lab4: llama.cpp、RPC 与 Ray 推理实验

本目录完成课程 Lab4 的两条主线：

- `llama.cpp`：本地 GGUF 推理、性能指标、参数优化、质量测试与双机 RPC。
- Ray：单机多进程模拟异构推理节点，比较批量调度、负载均衡、并发和故障重试。

正式模型推理由 `llama.cpp` 提供；实验采集和数据处理以 Rust 工具为主。Ray 官方
Task API 没有受支持的 Rust SDK，因此 Ray 调度部分使用 Python，其他实验工具遵守
[`docs/src/team/conventions/rust.md`](../docs/src/team/conventions/rust.md)。

## 快速检查

```bash
git submodule update --init --recursive

sha256sum data/models/qwen3.5-2b-q4_k_m.gguf
# 57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8

cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

UV_CACHE_DIR=/tmp/dipecs-uv-cache \
  PYTHONPATH=scripts \
  uv run python -m unittest discover -s scripts/tests -v
```

模型文件不进入 Git。模型、llama.cpp 版本和构建步骤见
[llama.cpp 接入说明](docs/llama-cpp-setup.md)。

## 实验入口

参数优化：

```bash
UV_CACHE_DIR=/tmp/dipecs-uv-cache uv run python scripts/param_optimization.py
```

Ray 基础实验使用 20 条 Prompt：

```bash
.venv/bin/python scripts/ray_batch_inference.py \
  --prompt-count 20 \
  --output data/results/ray-batch-results.jsonl
```

Ray 负载均衡加分实验使用 30 条 Prompt：

```bash
.venv/bin/python scripts/ray_batch_inference.py \
  --prompt-count 30 \
  --strategies round_robin latency_based \
  --output data/results/ray-loadbalance-30/ray-loadbalance-30-detail.jsonl
```

Ray 实验应直接使用 `.venv/bin/python`。在当前 Ray/uv 组合中，`uv run` 会自动创建
runtime environment 并复制 working directory，可能在 16 GiB 主机上触发 OOM。

## 报告索引

### AI 审计日志（证据链）

- [本地推理实验](ai-log/01-local-inference.md)：工具、命令、指标与证据位置
- [RPC 双机实验](ai-log/02-rpc-distributed.md)：网络探测、设备发现、OOM 定位
- [Ray 调度实验](ai-log/03-ray-scheduling.md)：集群启动、策略对比、故障重试

### 正式实验报告

- [参数优化报告](reports/param-optimization-report.md)
- [RPC 双机实验报告](reports/rpc-experiment-report.md)
- [Ray 基础实验报告](reports/ray-experiment-report.md)
- [Ray 加分实验报告](reports/ray-bonus-report.md)
- [并发压力测试报告](reports/concurrency-stress-report.md)

### 辅助文档

- [任务拆解与完成状态](docs/task-breakdown.md)
- [OS 与分布式系统知识点](docs/os-knowledge.md)
- [llama.cpp 接入说明](docs/llama-cpp-setup.md)
- [RPC 双机操作手册](docs/rpc-two-machine-setup.md)
- [Rust 实现约束](docs/rust-implementation.md)

### 截图

- `assets/01-local-inference.png` — 本地 llama-cli 推理
- `assets/02-rpc-device-discovery.png` — RPC 设备发现
- `assets/03-rpc-inference.png` — RPC 成功推理
- `assets/04-ray-status.png` — Ray 集群状态
- `assets/05-ray-experiment-results.png` — Ray 实验结果

### 历史基线（Qwen2.5-1.5B）

- [性能分析](reports/archive/performance-analysis.md)
- [输出质量评估](reports/archive/quality-evaluation.md)
- [单机冒烟记录](reports/archive/smoke.md)

原始 JSONL 和汇总 JSON 位于 `data/results/`。仓库不提供 Makefile；本项目入口使用
Cargo、CMake、uv 和普通 shell 命令。
