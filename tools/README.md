# Tools

本目录存放**测量、生成与查看工具**，它们需要真机/模拟器或特定输入才能产出/展示结构化数据。

## 子目录

- `collect/` — 手动测量脚本，产出 `data/evaluation/` 中的 JSON/MD 数据集。
- `generate/` — 合成数据生成工具，不依赖真机。
- `view/` — 数据可视化/调试工具，用于本地查看 trace、replay、audit 结果。

## 与 `scripts/` 的区别

- `tools/`：产出数据集/产物或可视化数据，通常由 CI 或研究者手动运行，结果进入
  `data/evaluation/` 并被 dataset tests 校验。
- `scripts/`：开发者日常辅助脚本（环境设置、检查、设备操作、文档构建），
  见 `scripts/README.md`。
