# Third-Party Sources

本目录保存 DiPECS 评估、对照实验和相关系统研究中引用的外部项目或数据源。
它们不是 DiPECS runtime 主链路的一部分；默认不会被 `dipecsd`、Android collector
或核心 Rust crates 直接打包进产品逻辑。

## 目录清单

| 路径 | 类型 | 作用 |
| --- | --- | --- |
| `LSApp/` | Git submodule | 外部移动应用使用序列数据集，用于 next-app prediction、LSApp standard/cold-start split、strong baseline 和动作净收益投影。 |
| `TracePilot/` | Git submodule | OSH-2026 TracePilot 项目源码，用作 Android eBPF、Perfetto、frame-centric jank 分析和调度 hint 设计的参考实现。 |
| `TracePilotData/` | 本仓库内数据说明/轻量快照 | TracePilot 相关实验数据清单，用于说明 camera、feed-scroll、QQ page switch 和 UI jank analysis 等场景的数据结构。 |

另有 `lab4/third_party/llama.cpp/`，它属于 `lab4/` 课程实验，不属于本目录管理的
DiPECS runtime / evaluation 第三方来源。

## LSApp

来源：`https://github.com/aliannejadi/LSApp`

DiPECS 用它作为真实移动 app 使用序列数据源。当前用途包括：

- 生成 `data/lsapp/lsapp.tsv` 派生数据。
- 评估 next-app prediction 的 standard / cold-start split。
- 提供 `ensemble`、`strong_predictive` 等 ranker 的 hit@1 / hit@3 / hit@5 输入。
- 与 Pixel 6a 上测得的 PreWarm / PrefetchFile hit-miss 成本结合，计算同预算
  projected net benefit。

常用准备命令：

```bash
git submodule update --init third_party/LSApp
bash tools/prepare-lsapp.sh
```

## TracePilot

来源：`https://github.com/OSH-2026/TracePilot.git`

TracePilot 是面向 Android 交互负载的 frame-centric 调度辅助系统。它通过 eBPF
采集调度、Binder、futex 等内核事件，并用 Perfetto FrameTimeline 作为帧边界和
jank ground truth。DiPECS 当前主要把它作为系统研究参考：

- 理解 Android jank 根因分析为什么需要 frame-centric 而不是 PID-centric。
- 参考 eBPF + Perfetto 的数据对齐和关键路径分析方法。
- 为后续 platform-signed / system-image 部署中的 LMKD、cgroup、Binder、ftrace
  观测路线提供设计材料。

TracePilot 不是当前 DiPECS 普通 app 形态的运行时依赖。

## TracePilotData

来源：来自 `OSH-2026/TracePilot` 项目的实验数据整理。

该目录当前只保留轻量数据清单/说明，用来描述 TracePilot 相关 Android 性能追踪
场景和文件格式，包括：

- camera 行为特征；
- Chrome feed-scroll ftrace / frame stats；
- QQ page switch 行为分析；
- UI jank graph critical path、hint、ML model 相关数据结构。

它用于研究背景和实验设计对齐，不作为 DiPECS CI 的强制输入。

## Submodule 初始化

初始化根 `third_party/` 下的 submodule：

```bash
git submodule update --init third_party/LSApp third_party/TracePilot
```

初始化仓库全部 submodule（包括 `lab4/third_party/llama.cpp`）：

```bash
git submodule update --init --recursive
```

## 维护约定

- 新增第三方项目时，优先使用 Git submodule，并在本 README 中说明来源、用途和是否进入运行时路径。
- 不要在 `third_party/` 提交未经脱敏的个人数据、私有 trace、token 或设备唯一标识。
- 引用第三方数据或代码时，应保留上游 README / citation / license 信息；若上游未提供明确许可证，报告和代码中只能按研究引用处理，避免复制进产品代码。
- DiPECS 评估中可提交派生、脱敏、可复现的数据集到 `data/evaluation/` 或 `data/traces/`；原始外部数据应尽量留在 submodule 或本地准备步骤中。
