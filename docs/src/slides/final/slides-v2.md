---
theme: touying
title: DiPECS — 面向 Android 本地上下文的智能决策与动作执行原型
info: |
  DiPECS final presentation v2
layout: cover
class: text-center
transition: slide-left
duration: 40min
drawings:
  persist: false
mdc: true
touying:
  preset: simple
  footer: DiPECS · Android 本地上下文智能决策原型
---

# DiPECS

## 面向 Android 本地上下文的智能决策与动作执行原型

Digital Intelligence Platform for Efficient Computing Systems

<div class="pt-10 text-sm opacity-60">
课程项目最终汇报 · 40 分钟版 v2 · 2026.07
</div>

---

# 目录

<div class="agenda-grid mt-7">

<div class="agenda-item">
  <div class="agenda-no">01</div>
  <div>
    <div class="agenda-title">背景、目标与贡献</div>
    <div class="agenda-desc">为什么移动端智能助手需要本地上下文，以及 DiPECS 的边界。</div>
  </div>
</div>

<div class="agenda-item">
  <div class="agenda-no">02</div>
  <div>
    <div class="agenda-title">系统架构与运行时管线</div>
    <div class="agenda-desc">Android Collector、Rust pipeline、daemon、action bridge 如何组成闭环。</div>
  </div>
</div>

<div class="agenda-item">
  <div class="agenda-no">03</div>
  <div>
    <div class="agenda-title">隐私边界与动作治理</div>
    <div class="agenda-desc">RawEvent 如何变成受控上下文，模型建议为什么不能直接执行。</div>
  </div>
</div>

<div class="agenda-item">
  <div class="agenda-no">04</div>
  <div>
    <div class="agenda-title">实验验证与结果</div>
    <div class="agenda-desc">E2E、隐私、延迟、replay、资源开销、policy 测试。</div>
  </div>
</div>

<div class="agenda-item wide">
  <div class="agenda-no">05</div>
  <div>
    <div class="agenda-title">Demo、限制与总结</div>
    <div class="agenda-desc">现场演示路径、可复现备用方案、当前完成度与后续工作。</div>
  </div>
</div>

</div>

---

# 汇报目标

这次报告回答四个问题：

1. 为什么移动端智能助手需要本地上下文？
2. 为什么本地上下文不能直接裸给模型？
3. DiPECS 如何把采集、决策、动作执行组织成受控系统？
4. 当前实验能证明什么，不能证明什么？

<div class="mt-8 p-4 border rounded text-sm">
核心观点：DiPECS 不是让模型获得更多权限，而是在本地建立隐私边界、策略边界和审计边界。
</div>

---
layout: section
---

# 01 背景、目标与贡献

---

# 背景：智能助手需要本地上下文

<div class="grid grid-cols-2 gap-6 mt-6 text-sm">

<div>

### 仅靠聊天输入不够

- 用户当前正在使用哪个 App
- 是否刚收到重要通知
- 当前网络、电量、屏幕状态如何
- 最近是否发生应用切换或文件相关事件

</div>

<div>

### 但本地上下文不能裸奔

- 通知文本可能包含验证码、联系人、聊天内容
- App 使用序列可能反映隐私习惯
- 设备状态会影响动作是否合适
- 自动执行必须有明确边界

</div>

</div>

<div class="mt-8 p-4 border rounded text-sm">
问题不是“能不能采集”，而是采集之后如何约束使用。
</div>

---

# 问题：直接交给模型有什么风险？

<div class="mt-5 text-sm compact-table">

| 本地数据 | 价值 | 风险 |
| --- | --- | --- |
| 通知标题和正文 | 判断外部打断和重要性 | 泄漏聊天、验证码、联系人 |
| App 使用序列 | 判断用户当前任务 | 暴露行为习惯和工作状态 |
| 屏幕、电量、网络、勿扰 | 判断动作是否合适 | 可能推断用户状态 |
| 自动动作接口 | 减少重复操作 | 误执行、越权、难追责 |

</div>

<div class="mt-6 p-4 border rounded text-sm">
所以我们需要的是“受控上下文”，不是“把手机状态全部发给 LLM”。
</div>

---

# 设计目标

<div class="mt-6 grid grid-cols-2 gap-4 text-sm">

<div class="p-4 border rounded">

### 上下文感知

把应用切换、通知、设备状态等信号统一成事件流。

</div>

<div class="p-4 border rounded">

### 本地优先

原始事件只在本地短路径内存在，模型只接收结构化摘要。

</div>

<div class="p-4 border rounded">

### 受控动作

决策模块只能提出建议，执行前必须经过本地策略检查。

</div>

<div class="p-4 border rounded">

### 可回放与可审计

同一份 trace 可以复现实验结果，动作路径可以生成 audit record。

</div>

</div>

---

# 当前主要贡献

<div class="mt-5 text-sm">

1. **Android Collector**：采集通知、应用切换、设备状态等本地事件。
2. **Rust pipeline**：统一事件类型、脱敏、窗口聚合、replay。
3. **决策路由**：支持本地规则和可选云端 LLM baseline。
4. **动作治理**：动作生命周期、策略检查、Android action bridge。
5. **实验验证**：隐私泄漏对比、延迟对比、资源开销、端到端采集与 replay。

</div>

<div class="mt-8 p-4 border rounded text-sm">
项目验证的是“本地上下文 → 受控决策 → 授权动作”的系统路径。
</div>

---

# 我们不做什么？

为了让边界清楚，当前版本不把以下内容作为目标：

- 不做完整商用 Android 助手；
- 不要求 root 或内核级 hook 才能展示核心链路；
- 不把隐私原文直接发送给云端模型；
- 不让模型绕过本地策略直接控制设备；
- 不把模拟器电池/温度数据包装成真机实测结论。

<div class="mt-8 p-4 border rounded text-sm">
这不是功能退让，而是课程项目中的工程取舍：先把可验证闭环做稳。
</div>

---
layout: section
---

# 02 系统架构与运行时管线

---

# 总体架构

<div class="diagram-placeholder">
  <div>
    <div class="placeholder-title">Android Collector → Rust Pipeline → Decision / Policy → Action Bridge</div>
    <div class="placeholder-subtitle">采集 → 标准化 → 脱敏 → 窗口聚合 → 决策建议 → 策略检查 → 授权执行 / replay audit</div>
  </div>
</div>

<div class="mt-4 text-sm opacity-75">
核心思想：观察、决策、执行三个能力分层，不让任何一层单独拥有全部权限。
</div>

---

# 模块职责

<div class="mt-4 text-sm compact-table">

| 模块 | 职责 |
| --- | --- |
| `apps/android-collector` | Android 侧采集和本地 JSONL 写盘 |
| `aios-collector` | tail Android JSONL，转换为内部事件 |
| `aios-core` | 隐私处理、上下文窗口、策略、动作生命周期 |
| `aios-agent` | 本地规则、云端 baseline、决策建议 |
| `aios-action` | action stub 与 Android bridge |
| `aios-daemon` | 持续运行的后台 pipeline |
| `aios-cli` | replay、audit、实验入口 |

</div>

---

# Android 数据入口

<div class="mt-4 text-sm compact-table">

| 数据源 | 当前状态 | 进入系统的事件 |
| --- | --- | --- |
| `UsageStatsManager` | 已接入 | App transition、screen state |
| `NotificationListenerService` | 已接入 | Notification posted / interaction |
| Device context heartbeat | 已接入 | 电量、网络、屏幕、勿扰模式 |
| AccessibilityService | 预览 / 筛选用途 | 不作为核心生产链路 |
| `/proc` 差分 | daemon 侧已接入 | Proc state change |
| Binder / fanotify | 接口预留 | 后续扩展 |

</div>

<div class="mt-5 text-sm opacity-75">
当前优先使用 Android 公开 API：可授权、可复现、不要求 root。
</div>

---

# 运行时管线

```text
CollectorEvent
  → RawEvent
  → SanitizedEvent
  → StructuredContext
  → DecisionRequest
  → ActionSuggestion
  → PolicyDecision
  → ActionExecutionRecord
```

<div class="mt-6 grid grid-cols-2 gap-4 text-sm">

<div class="p-3 border rounded">
原始事件不会直接进入模型。
</div>

<div class="p-3 border rounded">
模型输出不是执行权限。
</div>

<div class="p-3 border rounded">
策略检查是动作执行前的硬边界。
</div>

<div class="p-3 border rounded">
最终路径可以通过 audit record 复现。
</div>

</div>

---

# 窗口聚合

单个事件通常不足以表达用户处境，所以 DiPECS 使用时间窗口聚合。

<div class="mt-5 text-sm compact-table">

| 输入事件 | 聚合后的上下文 |
| --- | --- |
| 最近应用切换 | 当前任务状态 |
| 最近通知事件 | 是否存在外部打断 |
| 设备状态 | 是否适合执行动作 |
| 历史动作记录 | 是否需要避免重复执行 |

</div>

<div class="mt-8 p-4 border rounded text-sm">
窗口聚合的作用是把“事件流”变成“可决策上下文”。
</div>

---

# Replay / audit 为什么重要？

在线系统每次设备状态都可能不同，直接比较很难。

```text
固定 trace
  → 固定 pipeline
  → 固定输出
  → 固定 audit hash
```

<div class="mt-6 grid grid-cols-2 gap-4 text-sm">

<div class="p-3 border rounded">
Replay 让实验、回归测试和报告结果可复现。
</div>

<div class="p-3 border rounded">
Audit hash 用来捕获处理路径和输出状态的变化。
</div>

</div>

---
layout: section
---

# 03 隐私边界与动作治理

---

# 信任边界在哪里？

<div class="mt-5 text-sm compact-table">

| 区域 | 可能包含什么 | 能否给模型 |
| --- | --- | --- |
| RawEvent | 原始通知、原始 UI 文本、设备细节 | 不直接给 |
| SanitizedEvent | 脱敏后的事件和低敏字段 | 可进入本地 pipeline |
| StructuredContext | 长度、类型、语义 hint、状态摘要 | 可给决策模块 |

</div>

<div class="mt-8 p-4 border rounded text-sm">
关键点：模型看到的是摘要，不是完整原始隐私文本。
</div>

---

# 通知脱敏示例

原始通知可能包含：

```text
title: "Alice"
text: "验证码 123456，请勿泄露"
```

DiPECS 不保留完整原文，而保留低敏提示：

```text
title_hint: { length_chars, script, is_emoji_only }
text_hint:  { length_chars, script, is_emoji_only }
semantic_hints: [...]
```

<div class="mt-5 text-sm opacity-75">
这仍能表达“收到一条通知”，但不会把通知正文直接交给后续模型或审计流。
</div>

---

# 隐私实验结果

已有对比实验：

<div class="mt-5 text-sm compact-table">

| 指标 | 无 DiPECS / naive cloud prompt | 有 DiPECS |
| --- | ---: | ---: |
| 原始通知文本片段数 | 300 | 300 |
| 泄漏到模型输入 / 审计的数量 | 22 | **0** |
| Prompt / 模型输入大小 | 63178 bytes | 645 bytes |

</div>

<div class="mt-8 p-4 border rounded text-sm">
结论：DiPECS 把 raw notification PII 挡在本地隐私边界内，同时显著压缩模型输入。
</div>

---

# 决策路由

<div class="mt-5 text-sm compact-table">

| 路径 | 用途 |
| --- | --- |
| 本地规则 | 高频、低风险、确定性动作 |
| 可选云端 LLM | 复杂语义判断或 baseline 对比 |

</div>

核心原则：

- 本地规则不依赖网络，延迟低；
- 云端模型只能作为建议来源；
- 动作执行必须经过本地 policy。

---

# 模型只能提出建议

模型输出不是权限边界。一个动作真正执行前，需要回答：

1. 这个动作类型是否允许？
2. 当前设备状态是否适合？
3. 目标资源是否在允许范围内？
4. 是否会重复执行或造成副作用？
5. 是否能留下审计记录？

<div class="mt-8 p-4 border rounded text-sm">
这也是 DiPECS 与普通 LLM agent demo 的关键区别。
</div>

---

# 策略检查

`PolicyEngine` 对动作建议做二阶审查。

<div class="mt-5 text-sm compact-table">

| 规则类型 | 示例 |
| --- | --- |
| 风险等级 | 高风险动作默认拒绝 |
| capability | 中风险动作按能力配置决定 |
| 置信度 | 低置信度 intent 拒绝 |
| target 范围 | target 不在 context 中拒绝 |
| batch 限制 | 每批动作数受限 |

</div>

<div class="mt-6 p-4 border rounded text-sm">
即使本地或云端后端建议了动作，最终仍由本地策略决定是否允许执行。
</div>

---

# 动作生命周期

```text
Suggested
  → PolicyChecked
  → Approved / Denied
  → Dispatched
  → Succeeded / Failed
  → Audited
```

<div class="mt-6 grid grid-cols-2 gap-4 text-sm">

<div class="p-3 border rounded">
把“想做什么”和“真的做了什么”分开。
</div>

<div class="p-3 border rounded">
每个终态都可以进入审计记录。
</div>

</div>

---

# Android Action Bridge

当前 Android bridge 覆盖四类动作：

<div class="mt-5 text-sm compact-table">

| 动作类型 | 设备终态审计事件 | 设备确认延迟 |
| --- | --- | ---: |
| KeepAlive | `keep_alive_job_executed` | 21.3 ms |
| ReleaseMemory | `release_memory_completed` | 13.4 ms |
| PreWarmProcess | `own_resources_prewarmed` | 31.2 ms |
| PrefetchFile | `prefetch_succeeded` | 1.1 ms |

</div>

<div class="mt-5 text-sm opacity-75">
重点不是动作数量，而是 Rust → Android → handler → audit 的执行闭环。
</div>

---
layout: section
---

# 04 实验验证与结果

---

# 实验总览

<div class="mt-5 text-sm compact-table">

| 实验 | 目的 | 当前结果 |
| --- | --- | --- |
| emulator E2E | 验证 Android 采集到 replay 的闭环 | REAL 数据源通过 |
| privacy comparison | 验证脱敏边界 | 22 leaks → 0 leaks |
| latency comparison | 比较本地和云端决策 | `<0.1ms` vs `约 7s` |
| replay overhead | 验证核心管线吞吐 | 1631 events / 128ms |
| resource overhead | 评估 Android 端运行开销 | CPU < 1%，PSS +3.6–5.6MB |
| policy tests | 验证治理边界 | 20/20 pass |

</div>

---

# E2E 结果

最新一次 emulator E2E：

<div class="mt-5 text-sm compact-table">

| 项目 | 结果 |
| --- | --- |
| AVD | `dipecs_e2e` |
| Android API | 35 |
| 数据源 | REAL |
| rawEvent 行数 | 1 |
| replay | success |
| audit hash | `sha256:c99c471c...16d7` |

</div>

<div class="mt-6 p-4 border rounded text-sm">
这次不是 fallback 样本，而是模拟器真实通知事件进入了 pipeline。
</div>

---

# 延迟结果

<div class="mt-5 text-sm compact-table">

| 后端 | 均值 | p50 | p95 |
| --- | ---: | ---: | ---: |
| RuleBased | 0.00 ms | 0.00 ms | 0.02 ms |
| LocalEvaluator | 0.01 ms | 0.01 ms | 0.05 ms |
| CloudLLM / DeepSeek | 6958.16 ms | 7339.61 ms | 10050.08 ms |

</div>

<div class="mt-8 p-4 border rounded text-sm">
结论：高频、低风险的上下文响应应优先走本地规则；云端 LLM 更适合作为复杂语义判断的可选路径。
</div>

---

# Replay 吞吐结果

大 trace replay 指标：

<div class="mt-5 text-sm compact-table">

| 指标 | 数值 |
| --- | ---: |
| Trace 行数 | 2400 |
| Events ingested | 1631 |
| Wall time | 128.0 ms |
| Peak RSS | 10.77 MB |
| Throughput | 12742.2 events/s |
| Actions authorized | 206 |

</div>

<div class="mt-6 p-4 border rounded text-sm">
核心管线可以快速处理千级事件，适合用于批量评估和回归测试。
</div>

---

# 资源开销设置

最新 Android emulator 资源开销实验：

<div class="mt-5 text-sm compact-table">

| 项目 | 配置 |
| --- | --- |
| 设备 | Android emulator |
| 样本数 | 30 / mode |
| 采样间隔 | 10 s |
| 模式 1 | baseline_idle |
| 模式 2 | dipecs_observe_only |
| 模式 3 | dipecs_action_loop |

</div>

<div class="mt-6 p-4 border rounded text-sm">
注意：emulator 的电池和温度不能作为真机强结论；这里只使用 CPU、RSS、PSS、jank 作为主要实测指标。
</div>

---

# 资源开销结果

<div class="mt-5 text-sm compact-table">

| Mode | Avg CPU | Avg RSS | Avg PSS | Avg jank |
| --- | ---: | ---: | ---: | ---: |
| baseline_idle | 0.493% | 118.297 MB | 36.024 MB | 0.0% |
| observe_only | 0.387% | 125.870 MB | 39.629 MB | 0.0% |
| action_loop | 0.000% | 132.797 MB | 41.621 MB | 0.0% |

</div>

<div class="mt-6 grid grid-cols-2 gap-4 text-sm">

<div class="p-3 border rounded">
CPU 低于 1%，未观察到 UI jank 增加。
</div>

<div class="p-3 border rounded">
PSS 增加约 3.6–5.6 MB，比 RSS 更适合汇报实际内存开销。
</div>

</div>

---

# 治理与 policy 测试

`PolicyEngine` 当前 20 项测试全部通过。

覆盖规则：

- 高风险动作默认拒绝；
- 中风险动作按 capability 配置决定；
- 低置信度 intent 拒绝；
- target 不在 context 中拒绝；
- 每 batch 最大动作数限制；
- deferred urgency 过滤；
- FallbackNoOp 拦截 PreWarm。

<div class="mt-6 p-4 border rounded text-sm">
价值：模型建议即使存在，也必须经过本地治理边界。
</div>

---

# UX 实验现状与限制

已有 UX 实验分支和数据，但当前结论要谨慎使用：

- PreWarm 实验脚本存在顺序问题；
- 当前脚本可能在 `force-stop` 后抹掉 PreWarm 效果；
- ReleaseMemory 的 jank 结论需要重新校验；
- `gfxinfo` 可能存在累计统计问题。

<div class="mt-6 p-4 border rounded text-sm">
下一步应重做三组对照：cold baseline、service warm without prewarm、service warm with prewarm。
</div>

---

# 实验结论汇总

<div class="mt-5 text-sm compact-table">

| 问题 | 当前证据 |
| --- | --- |
| 能否跑通 Android → replay？ | emulator E2E 已通过，数据源 REAL |
| 是否减少隐私泄漏？ | naive 22 leaks vs DiPECS 0 leaks |
| 本地决策是否更快？ | `<0.1ms` vs cloud `约 7s` |
| 资源开销是否可接受？ | CPU < 1%，PSS +3.6–5.6 MB |
| 是否可复现？ | replay + audit hash |

</div>

---
layout: section
---

# 05 Demo、限制与总结

---

# Demo：在线 emulator E2E

现场演示推荐跑短链路：

```bash
source ~/.zshrc
./tests/scenarios/emulator-e2e.sh --auto
```

展示点：

1. 模拟器在线；
2. APK 安装；
3. 触发通知事件；
4. 拉取 trace；
5. replay 成功；
6. audit hash 生成。

---

# Demo：离线 replay 备用

如果现场模拟器不稳定，可以用固定 trace 做离线 replay：

```bash
cargo run -q -p aios-cli -- replay \
  data/traces/android_synthetic_large.redacted.jsonl \
  --output data/evaluation/demo.ndjson \
  --audit data/evaluation/demo.audit
```

<div class="mt-6 grid grid-cols-2 gap-4 text-sm">

<div class="p-3 border rounded">
优点：不依赖模拟器实时状态，输出稳定。
</div>

<div class="p-3 border rounded">
重点：展示同一套核心 pipeline 的可复现能力。
</div>

</div>

---

# 当前完成度

<div class="mt-5 text-sm compact-table">

| 部分 | 状态 |
| --- | --- |
| Android Collector | 已完成核心事件采集 |
| Rust pipeline | 已完成标准化、脱敏、聚合、replay |
| 决策路由 | 已有本地规则和云端 baseline |
| 动作治理 | 已有 action lifecycle、policy、bridge |
| 实验 | 已有 E2E、隐私、延迟、资源开销 |
| 文档 / 报告 | 正在整合最终结果 |

</div>

---

# 已知限制

1. emulator 不能代表真机电池和温度；
2. UX 实验脚本还需要修正后重跑；
3. Binder / fanotify 等底层能力目前是预留，不是核心展示链路；
4. 当前动作类型有限，重点是治理机制而不是动作数量；
5. 真实用户长期使用数据尚未收集。

<div class="mt-8 p-4 border rounded text-sm">
这些限制要主动讲清楚，避免把原型结果说成生产系统结论。
</div>

---

# 未来工作

短期：

- 清理实验产物，只保留成功记录；
- 修正 UX 实验方法学；
- 把最终报告和 PPT 中的实验口径统一；
- 增加更稳定的现场演示脚本。

中期：

- 真机资源开销测试；
- 更丰富的动作类型；
- 更严格的隐私策略测试；
- 更多真实场景 trace。

---

# 总结

DiPECS 验证了一条本地上下文智能系统路径：

```text
本地采集
  → 隐私脱敏
  → 结构化上下文
  → 决策建议
  → 本地策略检查
  → 授权动作
  → replay / audit
```

<div class="mt-6 p-4 border rounded text-sm">
最终结论：本地上下文可以增强智能助手，但必须通过隐私边界、策略边界和审计边界来约束使用。
</div>

---

# Q&A

<div class="mt-12 text-xl opacity-70">
谢谢
</div>

<div class="mt-8 text-sm opacity-80">
可准备问题：为什么不用纯云端 LLM？脱敏是否会损失决策能力？emulator 数据能说明什么？动作执行如何避免越权？
</div>

