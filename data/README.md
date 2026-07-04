# data/

DiPECS 的数据目录，按用途分四个子目录。每个子目录另有自己的 README 或索引。

| 子目录 | 内容 | 是否入库 |
|------|------|------|
| [`traces/`](traces/README.md) | replay / golden / policy 验证用的 JSONL 轨迹 fixture，以及端到端场景脚本的运行捕获产物 | 是（大文件走 Git LFS） |
| `evaluation/` | 评估产物，按实验族分子目录（见下） | 是（供 CI dataset 测试回归校验） |
| `schemas/` | 数据流 schema 文档：[`datapath.md`](schemas/datapath.md) 索引 + 三个分阶段文件 | 是 |
| `lsapp/` | LSApp 派生数据集（`lsapp.tsv`），由 `tools/prepare-lsapp.sh` 从 `third_party/LSApp` 生成 | 否（`.gitignore`，179MB 派生数据不入库） |

## `evaluation/` — 按实验族组织

评估产物按实验族分子目录，避免扁平堆放。带时间戳的是历史运行产物，固定名的是 canonical / 最新报告。

| 子目录 | 内容 | 对应采集工具 / 生产者 |
|------|------|------|
| `evaluation/next-app/` | next-app 预测评估报告（`lsapp-*.report.json`、`synthetic-next-app-v1.report.json`） | `aios-cli eval-next-app` / `benchmark-next-app` |
| `evaluation/ux-metrics/` | UX 启动延迟 / jank 测量 | `tools/collect/collect-ux-metrics.sh` |
| `evaluation/resource-overhead/` | CPU / RSS / PSS 开销 | `tools/collect/collect-resource-overhead.sh` |
| `evaluation/stability/` | 长跑内存稳定性 | `tools/collect/collect-stability.sh` |
| `evaluation/cloud/` | 云端 LLM 延迟 / 场景基准 | `aios-agent` cloud-llm live-API benches |
| `evaluation/e2e/` | 模拟器 / action-loop / 设备内 e2e 运行产物（`.md` / `.ndjson` / `.audit`） | `tests/scenarios/*.sh` |
| `evaluation/value-metrics-*.md` | 跨实验族的综合价值汇总（留在 `evaluation/` 根） | 手工汇总 |

> 采集脚本的默认 `OUT_DIR` 已指向对应子目录，新采集会自动归类。
> CI 通过 `crates/aios-cli/tests/*_dataset_test.rs` 读取这些 fixture 做阈值回归；
> `scripts/ci/check_action_value_quality.py --dir data/evaluation` 递归校验收益字段的 provenance。

## 相关文档

- [评估工具](../docs/src/evaluation/tools.md) — 各采集脚本的用法与输出
- [评估场景与数据集](../docs/src/evaluation/scenarios.md) — 两条评估线（离线回归 / 端到端场景）
- [Schema 参考](../docs/src/refs/schemas.md) — 数据类型与 schema 锚点
