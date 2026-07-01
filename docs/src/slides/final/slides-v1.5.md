---
theme: touying
title: DiPECS — 面向 Android 本地上下文的智能决策与动作执行原型
info: |
  DiPECS 40-minute final presentation draft
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
课程项目最终汇报 · 40 分钟版初稿 · 2026.07
</div>

---

# 汇报目标

这次报告要回答四个问题：

1. 我们为什么要做一个本地上下文智能系统？
2. DiPECS 的系统架构、数据流和安全边界是什么？
3. 它现在已经实现到什么程度？
4. 我们用哪些实验说明它是可运行、可审计、低开销的？

<div class="mt-8 p-4 border rounded text-sm">
核心观点：DiPECS 不是“把所有手机数据交给大模型”，而是把本地事件压缩成受控上下文，再通过策略边界决定是否执行动作。
</div>

---

# 40 分钟结构

| 部分 | 时间 | 重点 |
| --- | ---: | --- |
| 背景与问题 | 5 min | 为什么需要本地上下文，为什么不能裸给模型 |
| 目标与贡献 | 4 min | DiPECS 做什么，不做什么 |
| 系统架构 | 8 min | Android Collector、Rust pipeline、daemon、action bridge |
| 隐私与信任边界 | 7 min | RawEvent、SanitizedEvent、StructuredContext |
| 决策与动作治理 | 6 min | 本地规则、可选云端、policy check、audit |
| 实验与结果 | 7 min | E2E、隐私、延迟、资源开销、replay |
| 总结与展望 | 3 min | 完成度、限制、下一步 |

---
layout: section
---

# 01 背景与问题

---

# 移动端智能助手缺什么？

大模型擅长理解语言，但移动端智能助手还需要知道“用户当下处境”：

- 当前正在使用哪个 App；
- 是否刚收到重要通知；
- 屏幕、电量、网络、勿扰模式是否改变；
- 最近是否发生应用切换；
- 某个动作现在执行是否合适。

<div class="mt-8 p-4 border rounded">
仅靠用户在聊天框里输入的问题，无法稳定表达这些上下文。
</div>

---

# 直接把上下文交给模型有什么问题？

本地上下文很有价值，但也很敏感：

| 数据 | 潜在风险 |
| --- | --- |
| 通知标题和正文 | 可能包含验证码、联系人、聊天内容 |
| App 使用序列 | 反映用户习惯、工作状态、健康状态 |
| 屏幕和设备状态 | 可能推断用户是否可打扰 |
| 自动动作接口 | 如果失控，会造成错误操作或越权操作 |

所以问题不是“能不能采集”，而是“采集后如何约束使用”。

---

# 我们的问题定义

我们希望构建一个本地优先的智能决策原型：

1. 能从 Android 设备采集可授权的上下文信号；
2. 能把原始事件转换成低敏、结构化上下文；
3. 能基于上下文提出动作建议；
4. 能在执行动作前进行本地策略检查；
5. 能通过 replay 和 audit 复现处理过程。

<div class="mt-8 p-4 border rounded">
目标不是做一个完整手机助手，而是验证“上下文 → 决策 → 受控动作”的系统路径是否成立。
</div>

---

# 设计原则

| 原则 | 含义 |
| --- | --- |
| 本地优先 | 原始数据尽量停留在本地短路径内 |
| 最小必要 | 模型只拿到任务所需的摘要，不拿完整原文 |
| 权限分离 | 决策模块不能直接拥有动作执行权 |
| 可回放 | 同一份 trace 可以复现实验结果 |
| 可审计 | 动作建议、策略判断和执行记录可追踪 |

---
layout: section
---

# 02 项目目标与贡献

---

# DiPECS 做了什么？

DiPECS 是一个 Android 本地上下文智能决策原型。

主链路：

```text
Android 事件采集
  → Rust 入口标准化
  → 隐私脱敏与上下文聚合
  → 决策路由
  → 策略检查
  → 授权动作执行 / replay audit
```

它验证的是一个系统结构：本地上下文不是直接交给模型，而是先进入受控 pipeline。

---

# 我们不做什么？

为了让项目边界清楚，我们明确不把以下内容作为当前版本目标：

- 不做完整商用 Android 助手；
- 不要求 root 或内核级 hook 才能展示核心链路；
- 不把隐私原文直接发送给云端模型；
- 不让模型绕过本地策略直接控制设备；
- 不把模拟器电池数据伪装成真机实测。

<div class="mt-8 p-4 border rounded">
这个边界很重要：它决定了我们的实验重点是系统可信路径，而不是堆功能。
</div>

---

# 当前主要贡献

1. Android Collector：采集通知、应用切换、设备状态等本地事件；
2. Rust pipeline：统一事件类型、脱敏、窗口聚合、replay；
3. 决策路由：支持本地规则与可选云端 LLM baseline；
4. 动作治理：动作生命周期、策略检查、Android action bridge；
5. 实验验证：隐私泄漏对比、延迟对比、资源开销、端到端采集与 replay。

---
layout: section
---

# 03 系统架构

---

# 总体架构

```text
Android Collector
  ├─ UsageStatsManager
  ├─ NotificationListenerService
  └─ Device Context

Rust Workspace
  ├─ aios-collector  事件入口
  ├─ aios-core       隐私、窗口、策略、动作生命周期
  ├─ aios-agent      决策路由
  ├─ aios-action     动作执行适配
  ├─ aios-daemon     长期运行管线
  └─ aios-cli        replay / audit / 调试
```

---

# 模块职责

| 模块 | 职责 |
| --- | --- |
| `apps/android-collector` | Android 侧采集和本地 JSONL 写盘 |
| `aios-collector` | tail Android JSONL，转换为内部事件 |
| `aios-core` | 隐私处理、上下文窗口、策略、动作生命周期 |
| `aios-agent` | 本地规则、云端 baseline、决策建议 |
| `aios-action` | action stub 与 Android bridge |
| `aios-daemon` | 持续运行的后台 pipeline |
| `aios-cli` | replay、audit、实验入口 |

---

# Android 数据入口

| 数据源 | 当前状态 | 进入系统的事件 |
| --- | --- | --- |
| `UsageStatsManager` | 已接入 | App transition、screen state |
| `NotificationListenerService` | 已接入 | Notification posted / interaction |
| Device context heartbeat | 已接入 | 电量、网络、屏幕、勿扰模式 |
| AccessibilityService | 预览 / 筛选用途 | 不作为核心生产链路 |
| `/proc` 差分 | daemon 侧已接入 | Proc state change |
| Binder / fanotify | 接口预留 | 后续扩展 |

---

# 为什么优先选这些数据源？

选择标准：

- Android 公开 API 可获得；
- 用户可以授权；
- 可以在模拟器和开发机稳定复现；
- 足以展示“上下文感知”的系统价值；
- 不要求 root，不把项目卡在底层 hook 上。

<div class="mt-8 p-4 border rounded">
这也是课程项目中的工程取舍：先把可验证闭环做稳，再扩展更底层的数据源。
</div>

---

# 运行时主链路

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

每一层都减少一个风险：

- 原始事件不会直接进入模型；
- 决策结果不是执行结果；
- 策略检查是动作执行前的硬边界；
- audit record 记录最终路径。

---

# 窗口聚合

单个事件通常不够表达用户处境，所以 DiPECS 使用时间窗口聚合：

| 输入 | 聚合后的上下文 |
| --- | --- |
| 最近应用切换 | 当前任务状态 |
| 最近通知事件 | 是否有重要外部打断 |
| 设备状态 | 是否适合执行动作 |
| 历史动作记录 | 是否需要避免重复执行 |

<div class="mt-8 p-4 border rounded">
窗口聚合的作用是把“事件流”变成“可决策上下文”。
</div>

---

# Offline replay 为什么重要？

在线系统很难直接比较，因为每次设备状态都可能不同。

replay 提供了一个稳定实验入口：

```text
固定 trace
  → 固定 pipeline
  → 固定输出
  → 固定 audit hash
```

这让我们可以做回归测试、实验对比和报告复现。

---
layout: section
---

# 04 隐私与信任边界

---

# 信任边界在哪里？

DiPECS 把系统分成三类数据区域：

| 区域 | 可包含什么 | 能否给模型 |
| --- | --- | --- |
| RawEvent | 原始通知、原始 UI 文本、设备细节 | 不直接给 |
| SanitizedEvent | 脱敏后的事件和低敏字段 | 可进入本地 pipeline |
| StructuredContext | 长度、类型、语义 hint、状态摘要 | 可给决策模块 |

关键点：模型看到的是摘要，不是原始隐私文本。

---

# 通知脱敏示例

原始通知可能包含：

```text
title: "Alice"
text: "验证码 123456，请勿泄露"
```

DiPECS 不把这类字段直接送入决策模块，而是保留低敏提示：

```text
title_hint: { length_chars, script, is_emoji_only }
text_hint:  { length_chars, script, is_emoji_only }
semantic_hints: [...]
```

这样仍能表达“收到一条通知”，但不暴露完整内容。

---

# 隐私不是简单删除所有信息

如果删除所有信息，系统无法决策。

我们的做法是保留“决策所需的低敏摘要”：

| 原始信息 | 保留方式 |
| --- | --- |
| 文本内容 | 不保留原文 |
| 文本长度 | 保留 |
| 文本脚本类型 | 保留，如 Latin / Han |
| 是否图片通知 | 保留 boolean |
| 包名 | 可用于来源分类 |
| 具体通知 key / payload | 脱敏或置空 |

---

# 脱敏闸门

端到端脚本中加入了脱敏检查：

- trace 写盘后立即检查敏感字段；
- 如果发现未脱敏值，直接失败；
- 不允许带着疑似隐私泄漏进入 replay；
- 避免旧 APK 或脱敏回归污染实验结果。

<div class="mt-8 p-4 border rounded">
这不是展示性质的检查，而是实验数据进入仓库前的安全闸门。
</div>

---

# 隐私实验结果

已有对比实验：

| 方法 | 泄漏计数 |
| --- | ---: |
| naive baseline | 22 |
| DiPECS pipeline | 0 |

汇报口径：

> 与直接把上下文字段送入后续处理相比，DiPECS 通过本地脱敏和结构化摘要，把实验中的敏感字段泄漏降为 0。

---
layout: section
---

# 05 决策与动作治理

---

# 决策路由

DiPECS 支持两类决策路径：

| 路径 | 用途 |
| --- | --- |
| 本地规则 | 高频、低风险、确定性动作 |
| 可选云端 LLM | 复杂语义判断或 baseline 对比 |

核心原则：

- 本地规则不依赖网络，延迟低；
- 云端模型只能作为建议来源；
- 动作执行必须经过本地 policy。

---

# 为什么模型不能直接执行动作？

模型输出不是权限边界。

一个动作真正执行前，需要回答：

1. 这个动作类型是否允许？
2. 当前设备状态是否适合？
3. 目标资源是否在允许范围内？
4. 是否会重复执行或造成副作用？
5. 是否能留下审计记录？

---

# Action lifecycle

```text
Suggested
  → PolicyChecked
  → Approved / Denied
  → Dispatched
  → Succeeded / Failed
  → Audited
```

这个生命周期把“想做什么”和“真的做了什么”分开。

<div class="mt-8 p-4 border rounded">
这也是 DiPECS 和普通 LLM agent demo 的关键区别：我们把执行权放在本地系统边界内。
</div>

---

# Android Action Bridge

Android bridge 用于把 Rust 侧 action 转发到 Android 侧：

- KeepAlive；
- ReleaseMemory；
- PreWarmProcess；
- PrefetchFile；
- 其他动作类型预留。

当前阶段重点不是动作种类数量，而是验证：

1. Rust 侧能提出动作；
2. policy 能检查动作；
3. Android 侧能接收并执行 / 记录；
4. 全路径可审计。

---

# Replay audit

每次 replay 可以输出：

- replay 结果；
- action decision；
- audit log；
- audit hash。

端到端验收中得到：

```text
数据源: REAL
rawEvent: 1
replay: success
audit hash: sha256:c99c471c0da2ed289792cf737faeadd1bf4897cd644af0edb8c2406a06ae16d7
```

---
layout: section
---

# 06 实验与结果

---

# 实验总览

我们现在有五类实验结果：

| 实验 | 目的 |
| --- | --- |
| emulator E2E | 验证 Android 采集到 replay 的闭环 |
| privacy comparison | 验证脱敏边界是否有效 |
| latency comparison | 比较本地决策和云端 baseline |
| resource overhead | 评估 Android 端运行开销 |
| dataset / unit tests | 验证数据格式、边界条件和回归 |

---

# 端到端验证

最新一次 emulator E2E：

| 项目 | 结果 |
| --- | --- |
| AVD | `dipecs_e2e` |
| Android API | 35 |
| 数据源 | REAL |
| rawEvent 行数 | 1 |
| replay | success |
| audit hash | `sha256:c99c471c...16d7` |

说明：这次不是 fallback 样本，而是模拟器真实通知事件进入了 pipeline。

---

# E2E 中发现并修复的问题

端到端跑测时发现两个工程问题：

1. 复用已有 emulator 时，脚本没有等待 Android framework 完全启动；
2. 脱敏检查函数在“没有泄漏”时会被 `set -euo pipefail` 误杀。

修复后：

- 安装 APK 正常；
- 真实事件采集正常；
- replay 正常；
- audit hash 正常生成。

---

# 延迟实验

已有 value metrics 对比：

| 路径 | 延迟 |
| --- | ---: |
| 本地规则决策 | `< 0.1 ms` |
| DeepSeek / cloud baseline | `约 7 s` |

结论：

> 高频、低风险的上下文响应应该优先走本地规则；云端 LLM 更适合作为复杂语义判断的可选路径，而不是阻塞主链路。

---

# Replay 吞吐与内存

已有 replay 指标：

| 指标 | 结果 |
| --- | ---: |
| replay events | 1631 |
| replay time | 128 ms |
| RSS | 10.77 MB |

含义：

- 离线回放足够快；
- 可以支持批量评估和回归测试；
- audit 路径不会成为实验瓶颈。

---

# 资源开销实验设置

最新资源开销实验：

| 项目 | 配置 |
| --- | --- |
| 设备 | Android emulator |
| 样本数 | 30 / mode |
| 采样间隔 | 10 s |
| 模式 1 | baseline_idle |
| 模式 2 | dipecs_observe_only |
| 模式 3 | dipecs_action_loop |

注意：emulator 的电池和温度不能作为真机强结论。

---

# 资源开销结果

| Mode | Avg CPU | Avg RSS | Avg PSS | Avg jank |
| --- | ---: | ---: | ---: | ---: |
| baseline_idle | 0.493% | 118.297 MB | 36.024 MB | 0.0% |
| observe_only | 0.387% | 125.870 MB | 39.629 MB | 0.0% |
| action_loop | 0.000% | 132.797 MB | 41.621 MB | 0.0% |

报告口径：

> DiPECS 在 emulator 上 CPU 开销低于 1%，PSS 增加约 3.6–5.6 MB，未观察到 UI jank 增加。

---

# 如何解释 RSS 和 PSS？

| 指标 | 含义 | 报告中怎么用 |
| --- | --- | --- |
| RSS | 进程映射到内存中的总量 | 可作为上界参考 |
| PSS | 按比例分摊共享内存后的占用 | 更适合汇报实际内存开销 |

所以我们主要使用 PSS：

```text
baseline:    36.024 MB
observe:     39.629 MB  (+3.605 MB)
action loop: 41.621 MB  (+5.597 MB)
```

---

# 测试覆盖

当前已有测试覆盖：

- workspace Rust tests；
- Android JSONL parse tests；
- privacy / governance comparison；
- resource overhead dataset tests；
- daemon worker tests；
- action latency smoke；
- emulator E2E script。

最近资源开销数据集测试：

```text
5 passed; 0 failed
```

---

# UX 实验的当前状态

已有 UX 实验分支和数据，但当前结论要谨慎使用：

- PreWarm 实验脚本存在顺序问题；
- 当前脚本可能在 `force-stop` 后抹掉 PreWarm 效果；
- ReleaseMemory 的 jank 结论也需要重新校验；
- `gfxinfo` 可能存在累计统计问题。

下一步应该重做三组对照：

1. cold baseline；
2. service warm without prewarm；
3. service warm with prewarm。

---

# 实验结论汇总

| 问题 | 当前证据 |
| --- | --- |
| 能否跑通 Android → replay？ | emulator E2E 已通过，数据源 REAL |
| 是否减少隐私泄漏？ | naive 22 leaks vs DiPECS 0 leaks |
| 本地决策是否更快？ | `<0.1 ms` vs cloud `约 7 s` |
| 资源开销是否可接受？ | CPU < 1%，PSS +3.6–5.6 MB |
| 是否可复现？ | replay + audit hash |

---
layout: section
---

# 07 Demo 路径

---

# 推荐现场演示

现场演示不要跑 10 分钟资源实验，推荐演示短链路：

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

# 备用演示：离线 replay

如果现场模拟器不稳定，可以用固定 trace 做离线 replay：

```bash
cargo run -q -p aios-cli -- replay \
  data/traces/android_synthetic_large.redacted.jsonl \
  --output data/evaluation/demo.ndjson \
  --audit data/evaluation/demo.audit
```

优点：

- 不依赖模拟器实时状态；
- 输出稳定；
- 适合课堂环境兜底。

---

# 展示时应该强调什么？

不要只展示“脚本跑过了”，要强调：

- 数据源从 Android 来；
- trace 写盘前已经脱敏；
- replay 可以复现；
- audit hash 证明输出路径稳定；
- action 不是模型直接执行，而是经过 policy。

---
layout: section
---

# 08 当前完成度、限制与未来工作

---

# 当前完成度

| 部分 | 状态 |
| --- | --- |
| Android Collector | 已完成核心事件采集 |
| Rust pipeline | 已完成标准化、脱敏、聚合、replay |
| 决策路由 | 已有本地规则和云端 baseline |
| 动作治理 | 已有 action lifecycle、policy、bridge |
| 实验 | 已有 E2E、隐私、延迟、资源开销 |
| 文档 / 报告 | 需要继续整合最终结果 |

---

# 已知限制

1. emulator 不能代表真机电池和温度；
2. UX 实验脚本还需要修正后重跑；
3. Binder / fanotify 等底层能力目前是预留，不是核心展示链路；
4. 当前动作类型有限，重点是治理机制而不是动作数量；
5. 真实用户长期使用数据尚未收集。

---

# 下一步工作

短期：

- 清理实验产物，只保留成功记录；
- 修正 UX 实验方法学；
- 补齐最终报告和 PPT；
- 把实验结论写成统一表格。

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

最终结论：

> 本地上下文可以增强智能助手，但必须通过隐私边界、策略边界和审计边界来约束使用。

---

# Q&A

## 谢谢

可以重点准备的问题：

- 为什么不用纯云端 LLM？
- 脱敏是否会损失决策能力？
- emulator 数据能说明什么，不能说明什么？
- 动作执行如何避免越权？
- 这个系统和普通 agent demo 的区别是什么？

