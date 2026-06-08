# Lab4 llama.cpp Smoke Report

## 总览

本次 smoke 于 2026-06-06 完成，目标是验证：

- `llama.cpp` CPU 构建可运行。
- Qwen2.5 1.5B Q4_K_M GGUF 可加载并生成中文文本。
- Rust `lab4-bench` 能批量调用 `llama-cli` 并保存完整输出。
- `llama-bench` 能输出 prompt processing 和 token generation 基线。

本报告只证明实验链路可用，不作为最终参数优化或 RPC 性能结论。

## 环境

| 项目 | 值 |
| :--- | :--- |
| OS | Arch Linux |
| Kernel | 7.0.11-arch1-1 |
| Architecture | x86_64 |
| CPU | 13th Gen Intel Core i7-13700H |
| Logical cores | 20 |
| Memory | 15,981,548 KiB |
| llama.cpp commit | `c4a278d68efa17811006f2123a84081dac03fac7` |
| llama.cpp build | `b9533-c4a278d68` |
| Backend | CPU |

## 模型

| 项目 | 值 |
| :--- | :--- |
| Model | Qwen2.5-1.5B-Instruct-GGUF |
| Quantization | Q4_K_M |
| File | `qwen2.5-1.5b-instruct-q4_k_m.gguf` |
| File size | 1,117,320,736 bytes |
| SHA-256 | `6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e` |

## 单条推理

命令：

```bash
lab4/third_party/llama.cpp/build/bin/llama-cli \
  -m lab4/data/models/qwen2.5-1.5b-instruct-q4_k_m.gguf \
  -p "用中文简要解释操作系统页缓存的作用。" \
  -n 64 \
  -t 8 \
  -c 1024 \
  -b 512 \
  --seed 42 \
  --temp 0.2 \
  --single-turn \
  --no-display-prompt \
  --simple-io \
  --show-timings
```

结果：

- 进程退出码为 0。
- 模型成功生成中文页缓存说明。
- Prompt processing：约 232.1 token/s。
- Generation：约 41.9 token/s。

## 五条 Prompt Smoke

Rust 工具依次执行 `quality-prompts.jsonl` 中的 5 条 prompt，每条请求独立启动并加载模型。

| Case | Exit code | Duration ms | Generation token/s |
| :--- | ---: | ---: | ---: |
| `smoke-llama-cpp-001` | 0 | 4529 | 42.6 |
| `smoke-llama-cpp-002` | 0 | 1992 | 43.5 |
| `smoke-llama-cpp-003` | 0 | 5107 | 42.6 |
| `smoke-llama-cpp-004` | 0 | 5108 | 42.1 |
| `smoke-llama-cpp-005` | 0 | 5501 | 42.4 |

汇总：

- 成功数：5/5。
- 平均端到端耗时：4447.40 ms。
- 平均 generation throughput：42.64 token/s。
- 原始数据：`lab4/data/results/smoke-llama-cpp-quality.jsonl`。

端到端耗时包含每条请求的进程启动和模型加载，不等同于常驻服务的单请求延迟。

## llama-bench 基线

命令：

```bash
lab4/third_party/llama.cpp/build/bin/llama-bench \
  -m lab4/data/models/qwen2.5-1.5b-instruct-q4_k_m.gguf \
  -t 8 \
  -p 128 \
  -n 64 \
  -r 3 \
  -o jsonl
```

保存结果：

| Test | Average token/s | Standard deviation |
| :--- | ---: | ---: |
| Prompt processing, 128 tokens | 240.97 | 2.48 |
| Token generation, 64 tokens | 46.22 | 0.44 |

原始数据：`lab4/data/results/smoke-llama-bench.jsonl`。

## 后续状态

线程数、`mmap`、batch、context 和温度对比已经完成，结果见
`lab4/reports/performance-analysis.md` 与 `lab4/reports/quality-evaluation.md`。

当前仍需：

1. 补充峰值 RSS 和真实首 Token 延迟。
2. 在第二台机器启动 `rpc-server`，完成真实 RPC smoke。
3. 部署 Ceph 并比较本地路径与共享存储路径。
