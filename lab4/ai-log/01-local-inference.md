# AI 审计日志：本地 llama.cpp 推理实验

## 实验目标

验证 llama.cpp CPU 后端在本机（Intel i7-13700H）上的部署可行性，测量 Prompt/Generation 吞吐、启动耗时，并优化线程数与 batch size。

## 工具与版本

| 工具 | 版本/来源 | 用途 |
| :--- | :--- | :--- |
| `llama-cli` | llama.cpp b9533-c4a278d68 | 交互式推理与 smoke 测试 |
| `llama-bench` | 同上 | 标准化性能基准测量 |
| `lab4-bench` (Rust) | lab4-tools crate | 批量调用 llama-cli 并记录 JSONL |
| `sha256sum` | coreutils | 校验 GGUF 模型完整性 |

## 关键命令

### 模型校验

```bash
sha256sum data/models/qwen3.5-2b-q4_k_m.gguf
# 期望：57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8
```

### Smoke 推理（截图证据）

```bash
./build/bin/llama-cli \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  -p "操作系统页缓存有什么作用？用中文回答。" \
  -n 128 -t 8 -c 1024 \
  --seed 42 --temp 0.2 \
  --reasoning off --reasoning-budget 0 \
  --no-display-prompt --simple-io --show-timings
```

### 线程数扫描（llama-bench）

```bash
for t in 1 2 4 8 12 14 20; do
  ./build/bin/llama-bench \
    -m data/models/qwen3.5-2b-q4_k_m.gguf \
    -t "$t" -p 128 -n 64 -r 3 -o jsonl \
    >> data/results/threads-matrix.jsonl
done
```

### Batch 大小扫描

```bash
for b in 64 128 256 512 1024 2048; do
  ./build/bin/llama-bench \
    -m data/models/qwen3.5-2b-q4_k_m.gguf \
    -t 12 -p 512 -n 64 -b "$b" -r 3 -o jsonl \
    >> data/results/batch-matrix.jsonl
done
```

## 关注的指标/事件

| 指标 | 工具 | 系统含义 |
| :--- | :--- | :--- |
| Prompt throughput (pp t/s) | llama-bench | prefill 阶段矩阵计算与 batch 利用率 |
| Generation throughput (tg t/s) | llama-bench | 逐 token decode、内存带宽 |
| 启动耗时 | Rust lab4-bench | 进程启动 + 模型加载 + 首次 prefill |
| 标准差 | llama-bench -r 3 | 调度抖动与后台负载噪声 |

## 关键结论

1. **线程数最佳点为 12**：i7-13700H 是混合架构（6P+8E），20 逻辑核心不代表线性扩展；14 线程以上 decode 吞吐回退，20 线程稳定性显著恶化。
2. **batch 64 为局部最优**：继续增大 batch 不提高矩阵利用率，反而增加工作集和缓存压力。
3. **mmap 降低启动耗时约 25%**：warm-cache 环境下，`mmap` 短请求启动 676 ms 对比 `no-mmap` 904 ms。
4. **模型校验通过**：SHA-256 与预期一致，推理输出为连贯中文。

## 证据位置

| 证据 | 路径 | 说明 |
| :--- | :--- | :--- |
| 推理截图 | `assets/01-local-inference.png` | llama-cli 中文回答与 143.4/30.2 t/s |
| 线程数原始数据 | `data/results/threads-matrix.jsonl` | 1/2/4/8/12/14/20 线程 × 3 次重复 |
| Batch 原始数据 | `data/results/batch-matrix.jsonl` | 64~2048 batch × 3 次重复 |
| mmap 对比 | `data/results/mmap-matrix.jsonl` | mmap vs no-mmap 吞吐对比 |
| 启动耗时 | `data/results/startup-mmap.jsonl` | 短请求 1 token 启动耗时 |
| 汇总报告 | `reports/param-optimization-report.md` | 完整分析与推荐配置 |
