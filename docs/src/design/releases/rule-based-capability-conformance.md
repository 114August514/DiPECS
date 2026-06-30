# RuleBased 能力对齐 — 零误拒路径 (Zero-Denial Path)

> **PR**: `fix/rulebased` → `main`
> **日期**: 2026-06
> **模型版本**: `rule-based-v0.2` → `rule-based-v0.3`

---

## 背景

RuleBased 后端的能力声明为 `[NoOp, ReleaseMemory, KeepAlive]`，但 v0.2 的实现会在多种信号路径下生成 `PreWarmProcess` / `PrefetchFile` 动作。这些动作在策略引擎中被标注 `ActionCapabilityDenied` 后丢弃——形成**静默误拒**：规则引擎发出了它本不该发出的动作，策略引擎又将其逐字驳回。

v0.3 将规则引擎的设计原则从"发出去再被拒"切换为**"仅发出策略会通过的"**：规则引擎在生成阶段就自检能力声明，不发出未授权动作。

---

## 变更清单

### 删除的规则

| 信号 | 旧动作 | 原因 |
|:---|:---|:---|
| `InterAppInteraction(ActivityLaunch)` | `PreWarmProcess + KeepAlive` | 隐私空隙抹掉了 `source_package`（仅保 uid），无法确定启动目标；且 `PreWarmProcess` 不在能力范围内 |
| `FileActivity(*)` | `PrefetchFile` | `PrefetchFile` 不属于 RuleBased 能力范围，应留给 Cloud/LocalEvaluator 层 |

### 修改的规则

| 信号 | 旧动作 | 新动作 | 置信度 |
|:---|:---|:---|:---|
| 通知含 FileMention | `PreWarmProcess` | `KeepAlive` (文件来源 app 本身已存活，只需保温) | 0.70 |
| AppTransition.Foreground | `PreWarmProcess + KeepAlive` | `KeepAlive` (仅保温，前台 app 已运行) | 0.80 |
| 纯通知 (无文件) | — (无规则) | `KeepAlive` 首位通知来源 app (保温供用户即将打开) | 0.55 |

### 新增规则

| 信号 | 动作 | 触发条件 | 置信度 |
|:---|:---|:---|:---|
| `ProcessResource` 内存压力 | `ReleaseMemory(package)` | RSS ≥ 1024 MB 或 Swap ≥ 128 MB | 0.65 |

---

## 不变式

**能力一致性：RuleBased 路由上的任何窗口均不会产生被拒动作。**

该不变式由以下测试确证：

- `noop_rate_test.rs` — 13 个覆盖性信号模式的矩阵测试，每个窗口的 `denied` 列必须为空
- `replay_denial_golden_test.rs` — 正向拒绝计数探针已转为零拒绝 conformance 验证

若未来规则重新引入未授权动作，上述测试会立报失败。

---

## 审查要点

1. `memory_pressure` 阈值 (RSS 1 GB / Swap 128 MB) 为启发式值——需对照真实 ProcReader 轨迹验证
2. 纯通知 → KeepAlive (0.55) 未区分用户 Tap vs Dismiss——需 collect 侧保留交互动作信号后方可细化
3. ActivityLaunch 间隙待 collect 侧 uid→package 解析完成后重新评估
