# AI 审计日志：Ray 批量推理任务调度实验

## 实验目标

通过单机多进程模拟异构推理集群，验证 Ray Task API 在批量推理场景下的调度能力，比较串行、固定分区、轮询和延迟感知四种策略的吞吐与延迟。

## 工具与版本

| 工具 | 版本/来源 | 用途 |
| :--- | :--- | :--- |
| `ray` | Python 包 | 分布式任务调度 runtime |
| `llama-server` | llama.cpp b9533-c4a278d68 | 常驻 HTTP 推理后端 |
| `ray_batch_inference.py` | lab4/scripts/ | 批量调度实验脚本 |
| `ray_failover_test.py` | lab4/scripts/ | 故障重试加分实验 |
| `concurrency_stress_test.py` | lab4/scripts/ | 并发压力测试 |
| `curl` / `requests` | 标准库 | HTTP 调用 llama-server |

## 关键命令

### 启动 llama-server 实例

```bash
# s1：模拟较快节点
./build/bin/llama-server \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --port 8080 -c 1024 --threads 8 -n 96 --cache-ram 0

# s2：模拟较慢节点
./build/bin/llama-server \
  -m ../../data/models/qwen3.5-2b-q4_k_m.gguf \
  --port 8081 -c 1024 --threads 4 -n 96 --cache-ram 0
```

### 启动 Ray head

```bash
ray start --head \
  --port=6379 \
  --num-cpus=2 \
  --resources='{"server_s1": 1, "server_s2": 1}' \
  --object-store-memory=134217728 \
  --include-dashboard=false \
  --disable-usage-stats
```

### 查看集群状态（截图证据）

```bash
ray status
# Resources: 0.0/2.0 CPU, 0.0/1.0 server_s1, 0.0/1.0 server_s2
```

### 基础实验：20 条 Prompt × 4 种策略

```bash
.venv/bin/python scripts/ray_batch_inference.py \
  --prompt-count 20 \
  --output data/results/ray-batch-results.jsonl
```

### 负载均衡加分：30 条 Prompt

```bash
.venv/bin/python scripts/ray_batch_inference.py \
  --prompt-count 30 \
  --strategies round_robin latency_based \
  --output data/results/ray-loadbalance-30/ray-loadbalance-30-detail.jsonl
```

### 并发压力测试

```bash
.venv/bin/python scripts/concurrency_stress_test.py \
  --concurrency 1 2 4 \
  --output data/results/concurrency-stress/
```

## 关注的指标/事件

| 指标 | 采集方式 | 系统含义 |
| :--- | :--- | :--- |
| 总 wall time | Python `time.perf_counter()` | 批量请求端到端耗时 |
| 吞吐 (prompts/s) | `prompt_count / wall_time` | 任务级并行效率 |
| 吞吐 (tokens/s) | `total_output_tokens / wall_time` | 实际生成速率 |
| 平均延迟 | 单请求耗时均值 | 并发竞争下的单请求体验 |
| P95 延迟 | nearest-rank 法 | 长尾延迟 |
| 节点请求数 | Ray Task 返回的节点标签 | 负载均衡效果 |
| 失败重试数 | Ray 内部计数 | 故障恢复能力 |

## 关键结论

1. **并行减少总耗时但增加单请求延迟**：串行延迟最低（3204 ms），但 fixed_partition 和 round_robin 的总耗时分别为 57.61 s 和 57.82 s，均比 serial 的 64.08 s 低约 10%。
2. **两种简单并行策略表现接近**：fixed_partition 吞吐为 0.347 prompts/s，round_robin 为 0.346 prompts/s。latency_based 因 warm-up 探测成本未能在短批次中回收收益。
3. **线程数差异验证成功**：s1（8 threads）平均延迟比 s2（4 threads）低约 15-20%，验证了计算资源对推理速度的影响。
4. **故障重试 100% 成功**：kill s2 后，失败 Task 自动重试到 s1，最终 20/20 成功。
5. **并发压力揭示竞争效应**：并发度 4 时，两个 llama-server 共享同一物理 CPU，P95 延迟显著上升。

## 证据位置

| 证据 | 路径 | 说明 |
| :--- | :--- | :--- |
| Ray 状态截图 | `assets/04-ray-status.png` | 1 node + server_s1/s2 资源 |
| 实验结果截图 | `assets/05-ray-experiment-results.png` | 4 种策略对比汇总 |
| 基础实验原始数据 | `data/results/ray-batch-results.jsonl` | 20 prompt × 4 策略 = 80 条 |
| 基础实验汇总 | `data/results/ray-batch-results.summary.json` | 策略级统计 |
| 负载均衡数据 | `data/results/ray-loadbalance-30/` | 30 条 × 2 策略 |
| 故障重试数据 | `data/results/ray-failover/` | kill s2 后重试记录 |
| 并发压力数据 | `data/results/concurrency-stress/` | 并发度 1/2/4 |
| 汇总报告 | `reports/ray-experiment-report.md` | 基础实验完整分析 |
| 加分报告 | `reports/ray-bonus-report.md` | 负载均衡与故障重试 |
| 压力报告 | `reports/concurrency-stress-report.md` | 并发压力测试分析 |
