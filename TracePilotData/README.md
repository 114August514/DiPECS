# TracePilot 数据清单

来自 [OSH-2026/TracePilot](https://github.com/OSH-2026/TracePilot) 项目的 Android 性能追踪实验数据集。

## 目录结构

```
TracePilotData/
├── camera/                    相机应用行为分析
├── feed-scroll/               Chrome 信息流滑动（ftrace + 帧统计）
│   ├── baseline/              第一次采集
│   └── supplement-20260520/   第二次采集（2026-05-20）
├── qq-page-switch/            QQ 页面切换行为分析
└── ui-jank-analysis/          多场景 UI 卡顿根因分析
    ├── basic/                 简易帧/卡顿计数
    ├── page-switch/           页面切换场景（Run 2）
    ├── video/                 视频浏览场景
    ├── ml-model/              卡顿分类模型（Task 17）
    └── compare_report.json    跨场景对比
```

## 各场景说明

### 1. camera

相机应用的调度器行为特征。

| 文件 | 说明 |
|------|------|
| `camera_behavior_features.csv` | 每秒特征向量：事件数、唯一 CPU/命令/TGID 数、突发标记 |

### 2. feed-scroll

Chrome 信息流滑动性能，含内核 ftrace 数据。

**baseline**（第一次采集）：

| 文件 | 说明 |
|------|------|
| `feed_scroll_events_by_second.csv` | 每秒调度事件计数 |
| `feed_scroll_threads_summary.csv` | 线程级 CPU 占用汇总 |
| `chrome_scroll_topdown_summary.json` | 自顶向下分析：top 线程、唤醒/就绪延迟百分位 |
| `chrome_scroll_topdown_framestats.txt` | dumpsys gfxinfo 原始帧数据 |

**supplement-20260520**（第二次采集，数据更丰富）：

| 文件 | 说明 |
|------|------|
| `*_summary.json` | 自顶向下汇总（39.7 秒，17 个目标线程） |
| `*_frame_summary.json` | 帧/卡顿统计（118 帧，16.6ms 下卡顿率 100%） |
| `*_ftrace_summary.json` | Binder + DMA fence + 块 I/O 事件统计 |
| `*_events_by_second.csv` | 每秒调度事件 |
| `*_frames.csv` | 逐帧耗时数据 |
| `*_framestats.txt` | dumpsys gfxinfo 原始帧数据 |
| `*_ftrace.txt` | 内核 ftrace 原始数据（1818 条事件） |
| `*_threads_classified.csv` | 按子系统分类的线程 |
| `*_threads_scored.csv` | 按卡顿贡献度评分的线程 |
| `*_threads_score_summary.json` | 线程评分汇总 |
| `*_threads_classification_summary.json` | 线程分类汇总 |
| `*_threads_summary.csv` | 线程级 CPU 占用 |

### 3. qq-page-switch

QQ（`com.tencent.mobileqq`）页面切换时的调度器行为。

| 文件 | 说明 |
|------|------|
| `behavior_features.csv` | 每秒特征（578 行，其中 127 个 QQ 窗口） |
| `behavior_analysis_qq.csv` | QQ 专属突发分析结果 |
| `behavior_analysis_top_packages.csv` | 系统级事件量 top 包排行 |
| `behavior_analysis_summary.txt` | 可读分析报告 |

关键发现：QQ 主进程平均每秒 3 个事件，P90 突发阈值 36 事件/秒，共检测到 12 个突发窗口。

### 4. ui-jank-analysis

基于图方法（Graph-based Critical Path）的多场景 UI 卡顿根因分析。

**basic** — 简易帧/卡顿计数（无关键路径）：

| 文件 | 说明 |
|------|------|
| `frames.txt` | 原始帧数据（230 帧） |
| `result_py.json` | Python 分析结果（220/230 帧卡顿） |
| `py_success.txt` | 分析成功标记 |

**page-switch** — Run 2（会话 `93d39f8a...`）：

| 文件 | 说明 |
|------|------|
| `result.json` | 完整分析结果（1271 帧，卡顿率 96.6%） |
| `graph_topology.json` | 关键路径图（有向图） |
| `graph_subgraph.json` | 卡顿相关子图 |
| `hints.json` | 启发式根因提示 |
| `identity_map.json` | 线程/进程标识映射 |
| `frames.txt` | 原始帧数据 |
| `thermal_profile.txt` | 温控降频记录 |
| `analysis-report.md` | 可读分析报告 |

**video** — 视频浏览场景（会话 `93d39f8a...-107983`）：

文件结构与 page-switch 相同。2141 帧，卡顿率 71.2%，含 561 帧视频解码帧。

**ml-model** — 卡顿分类模型（Task 17）：

| 文件 | 说明 |
|------|------|
| `training-report.md` | 模型训练报告 |
| `data/learned_model.h` | 导出的决策树分类器（scikit-learn） |
| `data/learned_model_info.txt` | 模型元数据 |
| `scripts/train_jank_model.py` | 训练脚本 |
| `scripts/label_jank.py` | 卡顿帧标注 |
| `scripts/graph_features.py` | 从关键路径图提取特征 |
| `scripts/check_feature_importance.py` | 特征重要性分析 |
| `scripts/auto_label.py` | 自动标注流水线 |
| `scripts/suspect_frames.py` | 可疑帧检测 |
| `scripts/trace_label.py` | ftrace 标注工具 |

**compare_report.json** — 三个会话（page-switch Run 1 / Run 2 / video）的横向对比。

## 数据格式说明

- **时间窗口**：行为特征 CSV 使用 1 秒粒度
- **帧数据**：来自 Android `dumpsys gfxinfo framestats`
- **调度数据**：来自 eBPF sched 追踪点（sched_switch、sched_waking、sched_wakeup、cpu_frequency）
- **ftrace 数据**：内核 ftrace，记录 Binder 事务、DMA fence 等待、块 I/O
- **图格式**：`graph_topology.json` 使用 `tracepilot_graph_v1` 格式（有向图，节点 = 线程/任务，边 = 依赖关系）
