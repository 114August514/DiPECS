---
theme: touying
title: DiPECS — 面向 Android 的本地优先 AIOS 原型系统
info: |
  DiPECS final presentation · 40 minutes
layout: cover
class: text-center
transition: slide-left
duration: 40min
drawings:
  persist: false
mdc: true
touying:
  preset: simple
  footer: DiPECS · Local-First Android AIOS
---

# DiPECS

## 面向 Android 的本地优先 AIOS 原型系统

<div class="mt-7 text-lg opacity-80">
本地感知 · 资源预测 · 授权动作 · 真机收益
</div>

<!--
（封面页：先停 1 秒，面向老师。不要读副标题太快。）
各位老师、同学大家好，我们汇报的项目是 DiPECS。我们把它定位为一个面向 Android 平台的本地优先 AIOS 原型系统。今天汇报的核心不是某一个模型效果，也不是只做隐私保护，而是一条面向设备表现的 OS 控制闭环：本地信号怎样变成窗口级系统状态，预测怎样转化为受控资源动作，这些动作怎样在真机上带来可测的延迟或内存收益。
-->
---

# 一句话理解 DiPECS

<div class="mt-7 grid grid-cols-6 gap-2 text-center text-xs">
  <div class="p-3 border rounded"><div class="text-xl mb-2">①</div><b>Sense</b><br/>应用、通知、进程、设备</div>
  <div class="p-3 border rounded"><div class="text-xl mb-2">②</div><b>Sanitize</b><br/>Privacy Air-Gap</div>
  <div class="p-3 border rounded"><div class="text-xl mb-2">③</div><b>Context</b><br/>结构化时间窗口</div>
  <div class="p-3 border rounded"><div class="text-xl mb-2">④</div><b>Decide</b><br/>本地优先、可选 LLM</div>
  <div class="p-3 border rounded"><div class="text-xl mb-2">⑤</div><b>Authorize</b><br/>策略审查后执行</div>
  <div class="p-3 border rounded"><div class="text-xl mb-2">⑥</div><b>Audit</b><br/>终态记录与回放</div>
</div>

<div class="mt-7 p-5 border rounded text-center text-lg">
探索智能操作系统中<strong>感知—预测—资源动作—性能收益</strong>的受控闭环
</div>

<!--
（手指从左到右扫过“感知、脱敏、上下文、决策、授权执行、审计”。这里慢一点。）
DiPECS 可以概括为六个环节：感知、脱敏、上下文、决策、授权执行、审计。这里隐私和审计不是最终卖点，而是动作能安全进入系统前必须有的边界。系统从应用切换、通知、设备状态、进程状态等 Android 本地信号采集事件，聚合成上下文窗口；规则、本地评估器或可选云模型只生成候选意图；真正动作必须经过 PolicyEngine 和 ActionLifecycle，形成 AuthorizedAction 后才执行。我们最终要验证的是这些受控动作能否改善性能和表现，例如减少冷启动等待、减少 miss fetch 等待、释放 app-owned volatile memory。
-->
---

# 目录

<div class="mt-8 text-2xl leading-12">

1. 问题、目标与定位
2. 系统架构与 OS 机制
3. 隐私、决策与动作治理
4. 闭环运行证据
5. 实验设计、结果与边界
6. 局限、未来工作与总结

</div>

<!--
（目录页只读一级目录，不展开细节。读完马上翻页。）
这次汇报按六部分展开：先讲问题、目标与定位；然后讲系统架构和 OS 机制；第三部分重点讲隐私、决策和动作治理；第四部分给闭环运行证据；第五部分讲实验设计、结果和边界；最后总结局限与未来工作。
-->
---
layout: section
---

# 01 问题、目标与定位

<!--
（章节页：停半秒，提示进入第一部分。）
第一部分先回答为什么这个问题值得做，以及 DiPECS 在课程大作业中的定位。
-->
---

# 从“AI 能力”到“Android 性能闭环”

<div class="grid grid-cols-2 gap-6 mt-6 text-sm">

<div class="p-5 border rounded">

### 系统侧问题

感知信号 → 模型建议 → 系统动作

- 本地信号分散在不同 OS 接口
- 冷启动、I/O miss、内存压力会影响用户可感知表现
- 模型输出不具备系统执行权限，也不能无预算地消耗资源
- 网络与模型失败不能阻塞即时性能路径

</div>

<div class="p-5 border rounded">

### DiPECS 的研究问题

能否把 AI 预测转化为可测的 Android 资源收益？

- 本地优先完成上下文构造、决策与降级
- 模型只产生 Intent，不直接操作系统
- 动作必须经过授权、执行和终态审计
- 用真机 hit/miss、压力和强基线验证收益

</div>

</div>

<div class="mt-6 p-4 border rounded text-center">
核心不是“让模型控制 OS”，而是把预测变成<strong>有预算、有授权、能量化收益</strong>的 Android 资源动作。
</div>

<!--
（先指左侧“系统侧问题”，再指右侧“研究问题”，最后指底部核心句。）
现在大模型越来越多地进入应用和系统服务，可以总结通知、理解意图、预测下一步操作。但操作系统层面关心的不只是“能不能理解”，而是理解之后能不能改善设备表现：冷启动等待能不能下降，I/O miss 能不能减少，内存压力下能不能释放有用资源。DiPECS 的研究问题是：能否把 AI 预测转化为有预算、有授权、能量化收益的 Android 资源动作。隐私、策略和审计是必须的系统边界，但最终价值必须落到性能和表现上。
-->
---

# 典型场景：附件通知触发受控资源准备

<div class="mt-7 grid grid-cols-4 gap-3 text-center text-sm">
  <div class="p-4 border rounded"><b>t₀</b><br/><br/>收到聊天应用通知<br/>包含文件语义</div>
  <div class="p-4 border rounded"><b>t₀ + Δ</b><br/><br/>脱敏上下文判断<br/>可能打开该应用</div>
  <div class="p-4 border rounded"><b>策略门</b><br/><br/>检查目标、风险、<br/>能力与置信度</div>
  <div class="p-4 border rounded"><b>t₁</b><br/><br/>授权低风险准备<br/>或安全地 NoOp</div>
</div>

<div class="mt-7 grid grid-cols-3 gap-4 text-sm">
  <div class="p-3 border rounded"><b>隐私约束</b><br/>通知正文不进入模型</div>
  <div class="p-3 border rounded"><b>安全约束</b><br/>模型不能直接调用执行器</div>
  <div class="p-3 border rounded"><b>资源约束</b><br/>低置信度时不执行</div>
</div>

<!--
（按图从左到右讲：通知、脱敏、上下文、候选动作、策略、授权动作。）
用一个场景帮助理解：用户收到聊天软件通知，通知里可能有附件语义。系统不会把通知原文交给模型，而是在本地提取“可能有文件”“可能来自某个应用”这样的结构化提示。模型或规则可以建议预取文件、预热资源，但这个建议必须经过策略检查：目标是否在当前上下文里，风险等级是否可接受，置信度是否足够，动作数量是否超限。检查通过后才形成 AuthorizedAction；不通过就拒绝或 NoOp。
-->
---

# 项目目标与非目标

<div class="grid grid-cols-2 gap-6 mt-5 text-sm">

<div>

### 本项目要验证

- Android 与 Linux 信号能否形成统一上下文
- 原始隐私能否在模型前被强制隔离
- 多级决策能否退化到确定性本地路径
- 动作能否经过策略、认证和审计后执行
- 决策器在可观察上下文中能否给出可评测预测

</div>

<div>

### 当前版本不声称

- 不修改 Linux 内核、调度器或 Android LMKD
- 不把合成预测准确率外推为真实用户效果
- 不保证普通 APK 可执行所有系统级动作
- 不把模拟器功耗估算当作真机实测
- 不把未接入主链路的本地模型实验当成成品

</div>

</div>

<!--
（目标读完整；非目标用“边界”口吻讲，不要显得在道歉。）
项目目标有五个：统一 Android 本地信号，强制隔离原始隐私，多级决策可降级，动作经过策略、认证和审计，决策结果和动作收益可评测。
同时我们也明确边界：当前版本不修改内核、调度器或 Android LMKD；不把合成预测准确率外推成真实用户效果；不保证普通 APK 能执行所有系统级动作；不把模拟器功耗估算当作真机实测；也不把未接入主链路的本地模型实验包装成最终产品。
-->
---

# 三项核心贡献

<div class="mt-6 grid grid-cols-3 gap-5 text-sm">

<div class="p-5 border rounded">

### 1 · 本地感知上下文

将通知、应用、进程与设备状态标准化；原始文本和路径在 `PrivacyAirGap` 前后严格分界。

</div>

<div class="p-5 border rounded">

### 2 · 本地优先决策

用规则、本地评分和可选云模型覆盖不同复杂度；敏感、失败或离线时安全降级。

</div>

<div class="p-5 border rounded">

### 3 · 授权执行与审计

模型只提出候选；策略引擎、能力上限、HMAC 信封和生命周期共同形成可回放执行记录。

</div>

</div>

<div class="mt-7 p-4 border rounded text-center">
设计原则：<strong>机制与策略分离 · 最小权限 · fail closed · 可回放</strong>
</div>

<!--
（手指依次点三块贡献。）
DiPECS 的贡献可以归纳成三点。第一，本地感知上下文，把通知、应用、进程和设备状态标准化。第二，本地优先决策，用规则、本地评分和可选云模型覆盖不同复杂度，并在敏感、失败或离线时安全降级。第三，授权执行与审计，模型只提出候选，真正执行由策略、能力、HMAC 信封和生命周期共同约束。
-->
---
layout: section
---

# 02 系统架构与 OS 机制

<!--
（章节页：提示下面进入实现和 OS 机制。）
第二部分进入系统架构。这里重点讲 DiPECS 如何把数据面和控制面拆开，以及它对应了哪些 OS 课程里的机制。
-->
---

# 总体架构：本地优先 AIOS 数据面与控制面

<div class="diagram-frame">
  <img src="/diagrams/arch-overview.svg" />
</div>

<div class="mt-2 text-center opacity-75 text-sm">
Collector / Executor 提供机制，Router / Policy 决定策略；云模型不是可信计算基的一部分。
</div>

<!--
（核心图之一，慢讲。先指 Android 设备侧，再指 Rust 核心数据面，最后指控制面和审计。）
总体上，DiPECS 分成数据面和控制面。数据面负责把本地事件采集、校验、脱敏、聚合成上下文；控制面负责路由决策、策略审查、授权动作、设备执行和审计回放。
Android 侧用公开 API 采集应用切换、通知、设备状态，写入 app 私有目录下的 append-only JSONL trace。Rust daemon 侧 tail 这些 trace，同时读取 `/proc` 和系统状态。事件进入 RustCollectorIngress 做 schema 校验和来源标注，再经过 PrivacyAirGap、WindowAggregator 形成 StructuredContext。控制面从 DecisionRouter 开始，后端只产生 IntentBatch，PolicyEngine 和 ActionLifecycle 决定动作能否真正执行。
-->
---

# 部署与进程边界

<div class="mt-5 grid grid-cols-3 gap-5 text-sm">

<div class="p-4 border rounded">

### Android App 进程

- 公开 Android API 采集
- 私有 JSONL 持久化
- localhost action server
- 设备侧动作执行与审计

</div>

<div class="p-4 border rounded">

### `dipecsd` 进程

- `/proc` 与系统状态采集
- 脱敏、窗口、决策、策略
- 动作授权和回执处理
- SIGINT/SIGTERM 优雅退出

</div>

<div class="p-4 border rounded">

### 可选模型服务

- 本地规则默认可独立运行
- LLM 仅接收 Sanitized Context
- 不拥有设备执行权限
- 超时、异常时回退本地
- 默认可完全关闭

</div>

</div>

<!--
（指 Android App、Rust dipecsd、可选模型服务三层。）
部署上，Android App 负责通过公开 API 采集事件，写入私有 JSONL，并暴露 localhost action bridge。Rust `dipecsd` 负责 tail JSONL、读取 `/proc`、维护 channel、处理窗口、调用决策和策略。可选模型服务只接收 Sanitized Context，不接收原始文本。这个边界让采集、决策、执行和模型后端都能独立替换。
-->
---

# 模块依赖：稳定协议与入口层解耦

<div class="diagram-frame">
  <img src="/diagrams/module-deps.svg" />
</div>

<div class="diagram-caption">
`aios-spec` 固化跨模块类型、trait 与 IPC 协议；daemon / CLI 只是组合入口，不反向污染核心库。
</div>

<!--
（先指顶部 `aios-spec`，再指四个核心库，最后指 daemon/CLI。）
模块依赖上，我们把稳定协议放在 `aios-spec`。它定义跨模块类型、trait 和 IPC 协议。`aios-collector`、`aios-core`、`aios-agent`、`aios-action` 依赖 `aios-spec`，daemon 和 CLI 只是组合入口，不反向污染核心库。这体现机制和策略分离：采集器和执行器提供机制，Router 和 Policy 决定策略。
-->
---

# OS 概念映射：从 pipeline 到控制平面

<div class="mt-4 compact-table text-sm">

| OS 概念 | DiPECS 对应物 | 代码位置 |
|---|---|---|
| syscall ABI | Action Schema / `IntentBatch` | `aios-spec/src/intent.rs` |
| kernel policy engine | `PolicyEngine` | `aios-core/src/policy_engine.rs` |
| device driver | `ActionAdapter` trait | `aios-core/src/governance/mod.rs` |
| process trace / strace | Golden Trace replay | `aios-core/src/trace_engine.rs` |
| security boundary | `PrivacyAirGap` | `aios-core/src/privacy_airgap.rs` |
| capability system | `AuthorizedAction` | `aios-core/src/governance/mod.rs` |
| audit log | `AuditRecord` | `aios-spec/src/governance.rs` |
| scheduler / IPC | `ActionBus` + `DecisionRouter` | `aios-core/src/action_bus.rs` |

</div>

<div class="mt-5 p-4 border rounded text-center text-sm">
定位：<strong>DiPECS 不是普通 AI middleware，而是用户态 OS control plane 原型</strong>。
</div>

<!--
（这一页是老师最关心的 OS 映射页，不要讲成“我们也用了这些技术”。）
从 OS 课程角度看，DiPECS 不是普通 AI middleware，而是一个用户态 OS control plane 原型。IntentBatch 和 Action Schema 类似 syscall ABI，规定模型能提出什么系统调用形态；PolicyEngine 类似内核策略检查；ActionAdapter 类似 driver，把抽象动作映射到 Android 设备侧实现；Golden Trace replay 类似 strace/进程轨迹；PrivacyAirGap 是安全边界；AuthorizedAction 是 capability；AuditRecord 是审计日志；ActionBus 加 DecisionRouter 则承担 scheduler 和 IPC 的角色。
-->
---

# `/proc`：进程状态的内核接口

<div class="grid grid-cols-2 gap-6 mt-5 text-sm">

<div>

`ProcReader::scan_all()` 周期读取：

```text
/proc/<pid>/status
  VmRSS       常驻内存
  VmSwap      换出内存
  Threads     线程数
  Uid         所属用户

/proc/<pid>/oom_score
```

</div>

<div class="p-4 border rounded">

### 为什么做快照差分？

1. 本轮扫描构造 `pid → ProcSnapshot`
2. 与上一轮快照比较
3. 只为变化的进程生成事件
4. 减少后续窗口中的冗余信号

阈值规则还能识别高 RSS 或高 Swap 的内存压力候选。

</div>

</div>

<!--
（指 `/proc/<pid>/status`、RSS/Swap、oom_score 等位置。）
`/proc` 是 Linux 把内核进程状态暴露给用户态的接口。我们通过 `/proc/<pid>/status`、`oom_score` 等字段读取进程 RSS、Swap、线程数和 OOM 分数，并通过快照差分减少冗余事件。这里的关键点是低侵入观测：不需要修改内核，也能获得控制面所需的进程状态。
-->
---

# Daemon：进程与会话管理

<div class="diagram-frame">
  <img src="/diagrams/runtime-pipeline.svg" />
</div>

<div class="diagram-caption">
`fork + setsid + /dev/null` 完成 daemon 化；容量 4096 的 channel 解耦采集与处理，退出前刷新剩余窗口。
</div>

<!--
（先指 daemon 化，再沿运行时管线从左到右讲。）
`dipecsd` 使用 `fork + setsid + /dev/null` 完成 daemon 化，主进程退出，后台会话继续运行。运行时有两条任务：采集循环周期性 poll Android JSONL、ProcReader、SystemState 和可选 BinderProbe；处理循环从 `raw_events` channel 消费事件，经过 PrivacyAirGap、10 秒窗口聚合、Router、Lifecycle，最后产出 Audit Records 和 Runtime Trace。停机时采集侧 sender drop，处理侧看到 channel closed 后 flush 剩余窗口再退出。
-->
---

# Android OS 服务作为受控事件源

<div class="mt-4 compact-table text-sm">

| 数据源 | 事件 | 权限 / 约束 |
|---|---|---|
| `UsageStatsManager` | 应用前后台切换 | Usage Access app-op |
| `NotificationListenerService` | 通知发布与交互 | 用户显式启用监听 |
| `AccessibilityService` | 可选筛查 / 调查信号 | 用户显式授权；当前不进入 Rust 主链 RawEvent |
| Foreground Service | 5 s 轮询与 30 s heartbeat | 持续通知、生命周期约束 |
| `JobScheduler` | KeepAlive 维护任务 | 异步执行、系统统一调度 |

</div>

<div class="mt-5 p-4 border rounded text-sm">
选择公开 API 的原因：无需内核 hook，权限边界清晰，模拟器和真机都可复现；无障碍信号只作为可选辅助源处理。
</div>

<!--
（依次点 UsageStats、NotificationListener、前台服务、JobScheduler。Accessibility 只轻点一下。）
Android 侧我们有意识选择公开 API。`UsageStatsManager` 提供应用前后台切换，需要 Usage Access；`NotificationListenerService` 提供通知发布和交互，需要用户显式启用；前台服务提供轮询和 heartbeat；JobScheduler 用于 KeepAlive 维护任务。AccessibilityService 只作为可选筛查或调查信号，当前不进入 Rust 主链 RawEvent。这样做的好处是权限边界清晰，模拟器和真机都可复现。
-->
---

# Append-only JSONL 与增量 tail

<div class="diagram-frame">
  <img src="/diagrams/rust-ingress.svg" />
</div>

<div class="diagram-caption">
同一份 trace 同时服务于在线输入、离线 replay、回归测试与审计取证。
</div>

<!--
（指 append-only、byte offset、半行处理。）
Android app 每行写一个 CollectorEvent，Rust tailer 保存 byte offset，只解析包含 Rust-compatible `rawEvent` 的行。半行、文件截断和增量读取都要处理，否则在线输入和离线 replay 会不稳定。这个设计让同一份 trace 同时服务在线输入、离线 replay、回归测试和审计取证。
-->
---

# Replay / Audit：在线和离线共用核心逻辑

<div class="diagram-frame">
  <img src="/diagrams/replay-audit.svg" />
</div>

<div class="diagram-caption">
在线与离线只在适配器处不同；核心状态迁移和审计格式保持一致。
</div>

<!--
（从在线路径指到离线路径，最后指共同核心逻辑。）
Replay/Audit 的核心思想是在线和离线共用核心逻辑。在线路径经过 AndroidAdapter 执行真实设备动作；离线路径使用 OfflineAdapter，没有 I/O，行为确定，适合 golden hash。两者只在 adapter 处不同，核心状态迁移、策略审查和审计格式保持一致。
-->
---
layout: section
---

# 03 隐私、决策与动作治理

<!--
（章节页：提醒听众这是项目核心。）
第三部分是项目最核心的内容：隐私边界、决策路由和动作治理。这里要说明模型怎样参与，但不能越过系统授权。
-->
---

# Privacy Air Gap：模型前的强制边界

<div class="diagram-frame">
  <img src="/diagrams/privacy-boundary.svg" />
</div>

<div class="diagram-caption">
原始区与安全区以类型和依赖方向隔离；模型、上下文和审计只消费 `SanitizedEvent`。
</div>

<!--
（先指 RawEvent 原始区，再跨过 Air Gap 指 SanitizedEvent 和 StructuredContext。）
Privacy Air Gap 把系统分成原始区和安全区。原始区可以存在通知原文、文件路径、Binder 参数等 RawEvent；安全区之后只能出现 SanitizedEvent、StructuredContext、ModelInput 和 AuditRecord。模型、上下文和审计都只消费 SanitizedEvent。这个边界放在模型之前，所以它不是事后过滤，而是控制路径上的强制隔离。
-->
---

# 通知文本脱敏：从原始通知到语义提示

<div class="diagram-frame">
  <img src="/diagrams/semantic-hints.svg" />
</div>

<div class="diagram-caption">
Android 实采端先本地提取 hint 并清空原文；Rust PrivacyAirGap 作为模型前第二道强制边界，输出只保留统计量和类别提示。
</div>

<!--
（按两层讲：Android 端 hint 提取，Rust 端二次兜底。）
通知文本脱敏采用双层设计。Android 实采端先在本地从通知标题和文本中提取 `title_hint`、`text_hint` 和 `semantic_hints`，例如长度、脚本类别、是否包含文件、图片、验证码、金融语义等。写入 trace 时 raw title 和 raw text 已经为空。Rust 侧 PrivacyAirGap 是第二道强制边界，兼容旧 trace：如果旧数据里还有原文，Rust 会重新分析，只输出统计量和类别提示。最终原文、路径、通知 key、group key 都不进入模型输入。
-->
---

# 系统事件 → 窗口级上下文状态

<div class="diagram-frame">
  <img src="/diagrams/window-aggregation.svg" />
</div>

<div class="diagram-caption">
窗口结束后才触发一次决策；事件流先变成可推敲的系统状态，再进入决策器。
</div>

<div class="mt-3 compact-table text-xs">

| 步骤 | 抽象 | 构造 / 转化 | 设计收益 |
|---|---|---|---|
| Raw event | OS 事件样本 | app transition / notification / `/proc` / file signal | 保留来源、时间戳和 source tier |
| Sanitized event | 安全事件 | `PrivacyAirGap` 去除文本、路径和私有 key | 模型前强制隐私边界 |
| Window buffer | 时间局部状态 | `WindowAggregator` 收集 10 s 内事件 | 批处理噪声，降低触发频率 |
| StructuredContext | 窗口级系统状态 | 摘要、hint、foreground、资源压力、目标集合 | 供 Router、Policy、Audit 共用 |

</div>

<!--
（这页重点回答“如何抽象、如何构造、如何转化、为什么不是偷懒拼日志”。）
系统不会为每个噪声事件单独触发模型，而是把事件流抽象成窗口级系统状态。RawEvent 是 OS 事件样本，保留来源、时间戳和 source tier；PrivacyAirGap 把它转成 SanitizedEvent，去掉通知正文、路径和私有 key；WindowAggregator 在 10 秒内收集局部状态，窗口关闭时生成 StructuredContext。这个 StructuredContext 不只是日志集合，而是包含摘要、semantic hint、前台应用、资源压力和当前目标集合的状态对象。效率上，模型调用从 per-event 降到 per-window；策略上，PolicyEngine 可以据此判断动作目标是否真的出现在当前上下文里。
-->
---

# ActionBus：用户态 IPC 与 action syscall 队列

<div class="mt-5 grid grid-cols-2 gap-6 text-sm">

<div class="p-4 border rounded">

### 数据面 IPC

- `tokio::mpsc` 连接采集任务与处理任务
- bounded capacity = 4096
- sender drop 表示 shutdown
- 处理端退出前 flush 剩余窗口

</div>

<div class="p-4 border rounded">

### 控制面边界

- Raw events 只能进入脱敏和窗口管线
- Intent 只能进入 policy/lifecycle 管线
- 可执行对象必须是 `AuthorizedAction`
- 失败、超时、拒绝都有终态审计

</div>

</div>

<div class="mt-6 p-4 border rounded text-center text-sm">
ActionBus 是 model-native OS 的 syscall 边界雏形：模型建议进入队列，但执行必须经过本地 capability 和状态机。
</div>

<!--
（把 ActionBus 讲成 OS 边界，不讲成普通 Rust channel。）
ActionBus 不是为了工程上“传一下消息”这么简单。它把数据面和控制面分成两个通道：Raw events 只能进入脱敏和窗口管线，Intent 只能进入 policy/lifecycle 管线。bounded channel 提供背压，sender drop 表示 shutdown，处理侧在退出前 flush 剩余窗口。更重要的是，它定义了 action syscall 的边界：模型建议可以进入队列，但可执行对象必须由本地 capability 和生命周期状态机构造。
-->
---

# Scheduler：DecisionRouter 分级路由与熔断

<div class="diagram-frame">
  <img src="/diagrams/decision-routing.svg" />
</div>

<div class="diagram-caption">
把预测任务调度到规则、本地评估器、云端或 NoOp；连续错误按时间窗口计数，成功后清零。
</div>

<!--
（按 scheduler 语言讲，不要只说“选择模型”。）
DecisionRouter 在这里承担 scheduler 的角色：它把一个窗口级 StructuredContext 调度到规则、本地评估器、云端或 FallbackNoOp。优先级是：先看熔断状态，如果最近连续错误超过阈值，直接 FallbackNoOp；再看隐私敏感度，如果验证码或金融语义太多，阻止云端路径；再看本地可动作信号，比如文件访问、低电量、屏幕交互，这些直接走本地评估；最后才根据语义复杂度选择 RuleBased、LocalEvaluator 或 Cloud LLM。云端失败会退回本地规则，并记录错误，相当于控制回路里的故障隔离。
-->
---

# LocalEvaluator：可解释的确定性评分

<div class="mt-4 grid grid-cols-2 gap-6 text-sm">

<div>

```text
score = base
      + foreground_match
      + notification_file_correlation
      + repeated_package
      + behavior_memory
      - low_battery
      - cellular_or_offline
      - recent_policy_rejection
      - recent_execution_failure
```

</div>

<div class="p-4 border rounded">

### 输出约束

- 每窗口最多 5 个 intent
- 所有分数裁剪到 `[0, 1]`
- 只产生能力表允许的低风险动作
- 历史反馈只调整有限幅度
- 结果可重复，不依赖随机采样

</div>

</div>

<!--
（指特征输入、评分、结构化输出。）
LocalEvaluator 是一个可解释的确定性评分器。它把候选动作按显式特征加权，例如上下文里是否出现目标应用，是否有文件或图片语义，是否有近期执行失败，是否有低电量或系统压力信号。输出的目标、动作类型、置信度、风险等级都是结构化的，所以策略引擎可以继续审查。
-->
---

# PolicyEngine：action syscall 的能力检查

<div class="diagram-frame">
  <img src="/diagrams/policy-check-flow.svg" />
</div>

<div class="diagram-caption">
八项检查按 syscall-level capability gate 顺序 fail closed；全部通过才产生 `Approved`。
</div>

<!--
（逐项指 8 个检查框，不要展开每个实现细节；重点讲顺序执行和 fail closed。）
PolicyEngine 是模型建议到 action syscall 之间的硬边界。它按顺序做 8 项检查：后端能力等级、风险配置、置信度、batch 上限、阻止列表、延迟动作紧急度、动作能力白名单，以及目标是否在上下文中。任一失败都会拒绝，并写入 DenialReason。只有全部通过，才产生 `PolicyActionDecision::Approved`。这相当于在用户态实现 syscall-level capability checks。
-->
---

# ActionLifecycle：action syscall 状态机

<div class="diagram-frame">
  <img src="/diagrams/action-lifecycle.svg" />
</div>

<div class="diagram-caption">
只有 lifecycle 能构造 `AuthorizedAction`；连接、超时、设备拒绝与非法回执均进入可审计终态。
</div>

<!--
（沿状态机走：校验、Policy、seal、adapter、终态。）
ActionLifecycle 是 action syscall 的状态机。它保证每个动作恰好一个终态：先做 schema 校验，再调用 PolicyEngine，再 seal AuthorizedAction，再调用 adapter。如果 adapter 成功，记录 Succeeded；连接失败、超时、设备拒绝、非法回执都会进入 Failed；策略拒绝会进入 DeniedByPolicy 或相关终态。只有 lifecycle 能构造 AuthorizedAction，执行层不能自己伪造。
-->
---

# Android Action Bridge：认证与 freshness 约束

<div class="mt-4 grid grid-cols-2 gap-5 text-sm">

<div>

```text
BridgeExecuteRequest
├── message_type = execute
├── issued_at_ms
├── expires_at_ms = issued + 60 s
├── action = canonical AuthorizedAction JSON
└── auth.hmac_sha256
```

</div>

<div class="p-4 border rounded">

### HMAC 覆盖范围

```text
protocol version
+ issued_at_ms
+ expires_at_ms
+ length(action)
+ action bytes
```

因此旧标签不能替换动作，也不能跨过 freshness window 使用；当前原型未实现 nonce 级 replay cache。

</div>

</div>

<div class="mt-5 text-center text-sm opacity-75">
服务只绑定 `127.0.0.1:46321`，并对负载大小、读超时、并发队列和连续认证失败做限制。
</div>

<!--
（先指信封字段，再指 HMAC 覆盖范围，最后指 60 秒 freshness。）
Android Action Bridge 把授权动作送到设备侧。信封包含 `message_type = execute`、`issued_at_ms`、`expires_at_ms = issued + 60s`、canonical AuthorizedAction JSON，以及 HMAC。HMAC 覆盖 protocol version、issued time、expires time、action length 和 action bytes，所以旧标签不能替换动作，也不能跨过 freshness window 使用。当前原型还没有 nonce 级 replay cache，因此我们把能力边界表述为认证和时效窗口约束。
-->
---

# 动作与权限边界

<div class="mt-4 compact-table text-sm">

| Action | 设备侧实现 | 普通 APK / system app 边界 |
|---|---|---|
| `PreWarmProcess` | `own:*` 自有资源预热；`own:volatile-cache:<MB>` seed 可丢弃内存 | 后台启动第三方应用受限；系统签名更完整 |
| `PrefetchFile` | HTTPS / URI 预取到受控缓存 | 普通 APK 可执行自身可访问资源 |
| `KeepAlive` | OOM/cgroup 尝试 + JobScheduler fallback | OOM/cgroup 写入通常需要特权 |
| `ReleaseMemory` | `cache:prefetch` 清文件；`cache:volatile` 释放自有内存缓存 | 跨应用/全局回收需要系统权限 |
| `NoOp` | 确定性本地终态 | 安全退化路径 |

</div>

<div class="mt-5 p-4 border rounded text-sm">
同一动作在普通 APK 环境可能退化、被拒绝或只作用于自身资源；这属于 Android 权限模型，而不是隐藏为“成功”。
</div>

<!--
（依次点 PreWarm、Prefetch、KeepAlive、ReleaseMemory、NoOp。先讲边界，再讲收益证据。）
动作边界要讲清楚。`PreWarmProcess own:*` 预热 DiPECS 自身资源，普通 APK 对第三方后台启动会受限；`PrefetchFile` 可以预取 HTTPS 或 content URI 到受控缓存；`KeepAlive` 尝试 OOM/cgroup 并 fallback 到 JobScheduler，但普通 app 下机制会被 Android 生命周期管理限制；`ReleaseMemory` 的旧 `cache:prefetch` 只删磁盘缓存，升级后的 `cache:volatile` 释放 DiPECS 自己 seed 的可丢弃内存；`NoOp` 是确定性安全退化路径。不同权限环境下的退化、拒绝或只作用于自身资源，都会在审计中明确体现。
-->
---
layout: section
---

# 04 闭环运行证据

<!--
（章节页：从设计转到证据。）
第四部分讲系统跑通证据。我们准备了三类证据：输入链路、策略裁决和设备执行。
-->
---

# 案例输入：本地事件如何进入系统

<div class="diagram-frame">
  <img src="/diagrams/data-flow-e2e.svg" />
</div>

<div class="diagram-caption">
模拟器实采 smoke E2E：Android 35 公开 API 事件进入 replay/audit，并生成稳定审计哈希 `sha256:c99c471c…16d7`。
</div>

<!--
（从 Android 公开 API 指到 JSONL，再指 Rust replay/audit 和 hash。）
首先证明本地事件能进入系统。模拟器实采 smoke E2E 验证了 Android 35 公开 API 事件能写入 JSONL，被 Rust replay/audit 读取，并生成稳定审计哈希。这个证据不是大规模 workload，而是证明 Android 事件到 Rust 管线、再到审计 hash 的路径已经闭合。
-->
---

# 案例裁决：允许与拒绝

<div class="diagram-frame">
  <img src="/diagrams/action-governance.svg" />
</div>

<div class="diagram-caption">
高置信度不能绕过上下文事实；Policy 是独立于模型的硬边界。
</div>

<!--
（左边讲允许，右边讲拒绝；指拒绝原因时放慢。）
其次是案例裁决。即使模型或本地后端给出高置信度建议，也不能绕过上下文事实。如果目标不在当前窗口、能力等级不够、风险超过上限，Policy 就会拒绝。Policy 是独立于模型的硬边界，这一点是整个动作治理的关键。
-->
---

# 设备执行证据：模拟器 + Pixel 6a

<div class="mt-4 compact-table text-sm">

| 动作 | 设备终态审计事件 | 确认延迟 | 结果 |
|---|---|---:|---|
| KeepAlive | `keep_alive_scheduled → job_executed` | Pixel 6a 973 us | EXECUTED |
| ReleaseMemory | `release_memory_completed` | Pixel 6a 971 us | EXECUTED |
| PreWarmProcess | `own_resources_prewarmed` | Pixel 6a 841 us | EXECUTED* |
| PrefetchFile | `prefetch_started → prefetch_succeeded` | Pixel 6a 1964 us* | EXECUTED |

</div>

<div class="mt-5 grid grid-cols-2 gap-4 text-xs">
  <div class="p-3 border rounded">模拟器记录 `authorized_action_socket_execute_ok = 4`，四类处理器均留下终态；Pixel 6a 给出微秒级 action latency sweep。</div>
  <div class="p-3 border rounded">* PreWarm 验证 `own:warmup` / `own:*` 安全语义，不证明第三方静默预热；Prefetch 回执仅表示入队，最终下载由 `prefetch_succeeded` 取证。</div>
</div>

<div class="mt-5 text-xs opacity-70">
诚实边界：设备证据经与生产信封逐字节一致的取证发送器通过 adb forward 获得；它证明设备处理器执行，不等价于 daemon 已在设备内生产部署。Rust `AndroidAdapter` 另有 mock-socket E2E。
</div>

<!--
（指四类处理器终态，快速扫，不逐条读日志。）
第三是设备执行证据。我们验证了四类可转发动作处理器都有终态：KeepAlive 记录 `keep_alive_scheduled` 到 `job_executed`；ReleaseMemory 记录 `release_memory_completed`；PreWarmProcess 记录 `own_resources_prewarmed`；PrefetchFile 记录 `prefetch_started` 到 `prefetch_succeeded`。模拟器记录 `authorized_action_socket_execute_ok = 4`，Pixel 6a action latency sweep 显示四类设备回执在 841 到 1964 微秒。这里要诚实说明：这证明设备处理器执行，不等价于完整生产部署。
-->
---
layout: section
---

# 05 实验设计、结果与边界

<!--
（章节页：提醒每个结论都带证据等级。）
第五部分讲实验设计、结果和边界。这里我们把真实 API、模拟器实测、离线 replay 和估算值分开，不混用证据强度。
-->
---

# 实验问题与证据层级

<div class="mt-4 compact-table text-sm">

| RQ | 问题 | 证据 |
|---|---|---|
| RQ1 | 管线能否从 Android 事件走到可复现审计？ | 模拟器实采 smoke E2E + replay hash |
| RQ2 | 隐私边界是否阻止原始通知泄漏？ | naive prompt 与 DiPECS 对照 |
| RQ3 | 本地决策为何比云端更适合即时路径？ | 规则、本地、真实 API 延迟 |
| RQ4 | 当前决策器能否预测上下文支持的下一应用？ | 合成 trace + 派生 ground truth |
| RQ5 | 常驻开销是否可控？ | emulator / Pixel 6a CPU/RSS/PSS + replay 吞吐 |
| RQ6 | 动作治理是否覆盖主要拒绝路径？ | Policy 20 项测试 + action audit |
| RQ7 | 授权动作是否有真实收益？ | Pixel 6a n≥20 action-benefit gates |

</div>

<div class="mt-5 text-xs opacity-75">
证据标签：真实 API、模拟器实测、离线 replay、估算值分别标注，不混为同一强度结论。
</div>

<!--
（先指问题，再指证据类型。）
我们围绕七个问题组织实验：管线能否从 Android 事件走到可复现审计；隐私边界是否阻止原始通知泄漏；本地决策为何适合即时路径；当前决策器能否预测上下文支持的下一应用；常驻开销是否可控；动作治理是否覆盖主要拒绝路径；授权动作是否真的带来动作级收益。每个问题后面对应不同强度的证据，避免把 smoke E2E、合成数据和真机结论混在一起。
-->
---

# 合成预测评测：能力与覆盖边界

<div class="mt-4 grid grid-cols-2 gap-6 text-sm">

<div>

### 数据与标签

- 3 个确定性合成场景
- 10 s 上下文窗口，30 s 预测 horizon
- 总窗口：946
- 有未来切换：764
- 上下文可支持标签：178（23.3%）

</div>

<div class="compact-table">

| 后端 | Top-1 | Top-3 | 预测覆盖 |
|---|---:|---:|---:|
| RuleBased | **61.2%** | 65.7% | **93.8%** |
| LocalEvaluator | 43.8% | **62.9%** | 73.6% |

</div>

</div>

<div class="mt-5 p-4 border rounded text-sm">
结论边界：结果只描述<strong>合成、上下文可观察</strong>的切换；它验证评测框架与策略行为，不代表真实用户泛化准确率。RuleBased 的条件错误预测率仍为 34.7%。
</div>

<!--
（先指 946、764、178，再指 Top-1/Top-3，最后指 coverage 和错误率。）
合成预测评测来自三个确定性合成场景，使用 10 秒上下文窗口和 30 秒预测 horizon。总窗口 946，有未来切换 764，其中上下文可支持标签 178，占 23.3%。在这些 eligible 窗口上，RuleBased Top-1 是 61.2%，Top-3 是 65.7%，预测覆盖 93.8%；LocalEvaluator Top-1 是 43.8%，Top-3 是 62.9%，预测覆盖 73.6%。结论是：规则在合成、上下文可观察场景中能捕捉部分可解释模式，但不能外推到真实用户泛化准确率。
-->
---

# 决策延迟：本地即时，云端非即时

<div class="mt-5 compact-table text-sm">

| 后端 | p50 | p95 | 路径定位 |
|---|---:|---:|---|
| RuleBased | 0.00 ms | 0.02 ms | 高频、低风险 |
| LocalEvaluator | 0.01 ms | 0.05 ms | 无网络、可解释评分 |
| Cloud LLM（真实 API，2026-07-01 10 轮） | 7339.6 ms | 10050.1 ms | 复杂语义、非即时 |

</div>

<div class="mt-6 p-4 border rounded text-sm">
工程结论：即时资源动作默认本地；云端只适合作为可选分析路径，不能成为关键控制回路依赖。该数据只证明一次真实 API 延迟量级，不证明云端决策质量或稳定收益。
</div>

<!--
（先指本地两行，再指 Cloud LLM 行。讲 7 到 10 秒时停顿一下。）
决策延迟对比很直接：RuleBased p50 约 0.00 ms，p95 0.02 ms；LocalEvaluator p50 0.01 ms，p95 0.05 ms；Cloud LLM 使用 2026-07-01 的 10 轮真实 API 数据，p50 约 7.34 秒，p95 约 10.05 秒。结论是即时资源动作默认应该本地完成，云端只适合作为可选分析路径，不能成为关键控制回路依赖。
-->
---

# 隐私与治理结果

<div class="mt-4 grid grid-cols-2 gap-6 text-sm">

<div class="p-4 border rounded">

### Privacy comparison

| 指标 | Naive | DiPECS |
|---|---:|---:|
| 原始文本泄漏 | 22 | **0** |
| 输入大小 | 63,178 B | **645 B** |

审计流与 NDJSON 泄漏测试：2/2 pass。

</div>

<div class="p-4 border rounded">

### Governance coverage

- PolicyEngine：20 项测试通过
- target-in-context
- risk / capability 双重上限
- confidence floor
- batch cap 与 deferred filter
- FallbackNoOp 能力隔离

</div>

</div>

<!--
（先指 22 到 0 的泄漏对比，再指输入大小下降，最后指 PolicyEngine 20 项测试。）
隐私对照实验中，naive cloud prompt 会把原始通知文本直接拼进输入，检测到 22 个原始文本泄漏；DiPECS 路径为 0。输入大小从 63,178 bytes 降到 645 bytes。审计流与 NDJSON 泄漏测试 2/2 通过。治理覆盖方面，PolicyEngine 20 项测试通过，覆盖 target-in-context、risk/capability 双重上限、confidence floor、batch cap、deferred filter 和 FallbackNoOp 能力隔离。
-->
---

# 资源开销与离线吞吐

<div class="mt-4 grid grid-cols-2 gap-6 text-sm">

<div>

### Android emulator · 30 samples/mode

| 模式 | Avg PSS | 相对基线 |
|---|---:|---:|
| baseline | 36.024 MB | — |
| observe only | 39.629 MB | +3.605 MB |
| action loop | 41.621 MB | +5.597 MB |

</div>

<div>

### 2400-line replay

- 有效事件：1,631
- 完成窗口：58
- Wall time：128 ms
- Peak RSS：10.77 MB
- 吞吐：12,742 events/s
- 授权动作：206

</div>

</div>

<div class="mt-5 text-xs opacity-70">
CPU 采样存在粒度噪声；电池和温度为模拟器估算，本报告不将其作为实测功耗结论。
</div>

<!--
（指 PSS 增量，再指 replay 吞吐；CPU 和电池估算一句带过。）
资源开销方面，Android emulator 每个模式 30 个样本。baseline PSS 是 36.024 MB，observe only 是 39.629 MB，增加 3.605 MB；action loop 是 41.621 MB，增加 5.597 MB。离线 2400-line replay 中，有效事件 1631，完成窗口 58，wall time 128 ms，peak RSS 10.77 MB，吞吐 12,742 events/s，授权动作 206。CPU 采样有粒度噪声，电池和温度是模拟器估算，所以不作为真机功耗结论。
-->
---

# 动作收益与剩余边界

<div class="mt-5 grid grid-cols-2 gap-5 text-sm">

<div class="p-4 border rounded">

### 已闭环的动作级 gate

- PreWarm：Pixel 6a n=20/mode，710.75 → 201.55 ms，净收益高于强基线
- PrefetchFile：399 KB HTTPS 目标，79.993 ms hit read vs 1860.332 ms miss fetch+read
- ReleaseMemory：`cache:volatile` 真压力下 available +55 MB，PSS reduction +64 MB，p=0.00026891

</div>

<div class="p-4 border rounded">

### 仍不能外推

- 普通 APK 静默第三方预热
- KeepAlive 抗杀收益
- 模拟器电池/温度代表真机功耗
- Cloud LLM 能稳定产生有效即时动作
- 合成准确率代表真实用户行为

</div>

</div>

<!--
（主动讲，语气平稳。先讲三个正面 gate，再讲不能外推的边界。）
最后是动作收益与剩余边界。现在已有三个动作级正面 gate：PreWarm 在 Pixel 6a 上 n=20/mode，cold mean 710.75 ms、prewarm hit mean 201.55 ms，接入 LSApp standard 后 DiPECS 净收益 76,068,875 ms，高于强预测基线；PrefetchFile 在 Pixel 6a 上 n=20/mode，399 KB HTTPS 目标的命中读均值 79.993 ms，miss fetch+read 均值 1860.332 ms，投影净收益也高于强基线；ReleaseMemory 的旧磁盘缓存语义被降级，但 `cache:volatile` 在真压力下 available gain +55 MB、PSS reduction gain +64 MB，Welch p=0.00026891。仍不能外推的是：普通 APK 静默第三方预热、KeepAlive 抗杀收益、真机功耗、长期 field UX，以及 Cloud LLM 作为即时控制回路。
-->
---

# OS 资源管理视角

<div class="mt-5 grid grid-cols-3 gap-4 text-sm">

<div class="p-4 border rounded">

### Observability

- `/proc` 读取 RSS / swap / thread / oom score
- Android 服务提供 app / notification / device state
- action trace 记录设备侧终态

</div>

<div class="p-4 border rounded">

### Scheduling hints

- PreWarm：提前准备 app-owned resource
- Prefetch：预测命中时提前拉取 I/O
- ReleaseMemory：压力下释放 volatile cache
- KeepAlive：仅保留为系统部署前提

</div>

<div class="p-4 border rounded">

### Kernel authority

- 用户态只产生 hint 和授权动作
- Android sandbox / LMKD / scheduler 仍保留最终控制
- 失败时 NoOp / denial / audit

</div>

</div>

<div class="mt-6 p-4 border rounded text-center text-sm">
DiPECS 类似一个用户态 `sched_class` 扩展：用预测作为资源调度提示，但不取代内核和 Android 系统服务。
</div>

<!--
（这里把收益实验拉回 OS resource management，不要只说动作快了多少。）
这些动作可以从 OS 资源管理角度理解。DiPECS 先通过 `/proc` 和 Android 服务观测系统状态，再把模型或本地规则生成的预测转化为 scheduling hints：PreWarm 提前准备 app-owned resource，Prefetch 提前准备 I/O，ReleaseMemory 在压力下释放 volatile cache。关键点是 DiPECS 不取代内核权威：用户态只产生 hint 和授权动作，Android sandbox、LMKD 和 scheduler 仍保留最终控制；不满足条件时进入 NoOp、拒绝或审计终态。
-->
---
layout: section
---

# 06 局限、未来工作与总结

<!--
（章节页：开始收束，先讲设计取舍，再讲边界和下一步。）
最后一部分总结设计取舍、项目边界和下一步工作。
-->
---

# 为什么暂缓 eBPF：语义优先的 OS 设计选择

<div class="mt-5 grid grid-cols-2 gap-6 text-sm">

<div class="p-4 border rounded">

### eBPF / syscall trace 能回答

- 哪个进程调用了 `open()` / `read()` / `write()`
- 哪个线程发生了调度、Binder、futex 等事件
- 内核层事件的时间顺序与开销

</div>

<div class="p-4 border rounded">

### 但它不能单独回答

- 用户为什么打开这个文件
- 通知是否代表附件、验证码或金融语义
- 当前动作目标是否来自用户可见上下文
- 哪个 hint 应该被授权执行

</div>

</div>

<div class="mt-6 p-4 border rounded text-sm">
因此当前版本先选择 Android public API + `/proc` 的语义信号融合，把 Binder/eBPF 保留为未来 OS-level observability 扩展点。
</div>

<!--
（把没上 eBPF 讲成设计取舍，不是能力缺失。）
eBPF 或 raw syscall trace 很强，但它主要告诉我们 VFS、调度、Binder 层发生了什么，不能单独告诉我们用户意图是什么。拦截 open、read、write 可以看到哪个进程访问了文件，却不能区分“用户打开了 PDF”和“缓存守护进程写磁盘”；也不能判断通知是否表示附件、验证码或金融语义。所以当前版本先选择 Android public API 和 `/proc` 的语义信号融合，保证上下文能服务 PolicyEngine 和授权动作。Binder/eBPF 仍作为未来 OS-level observability 扩展点。
-->
---

# 项目边界：为什么是“用户态控制平面”

<div class="mt-5 grid grid-cols-2 gap-6 text-sm">

<div class="p-4 border rounded">

### 已实现

- 使用 OS 暴露的进程、文件、IPC 和服务接口
- 在用户态实现机制/策略分离
- 建立隐私、能力、策略、认证和审计边界
- 在模拟器和 Pixel 6a 上跑通设备动作处理器与授权回路
- 补齐 PreWarm、PrefetchFile、ReleaseMemory `cache:volatile` 的动作级收益证据

</div>

<div class="p-4 border rounded">

### 未深入内核

- 没有修改 scheduler、LMKD、VFS 或 Binder driver
- 普通 APK 受 Android 沙箱与后台启动限制
- Binder/fanotify 探针仍依赖特权部署
- 系统级动作需要 platform signing / ROM 集成

</div>

</div>

<div class="mt-6 p-4 border rounded text-center">
课程价值在于：用 OS 原则设计一个<strong>可运行、可降级、可追责</strong>的资源管理控制回路。
</div>

<!--
（左边讲已实现，右边讲未深入。不要把边界讲成缺陷。）
DiPECS 当前是用户态控制平面。已实现的是：使用 OS 暴露的进程、文件、IPC 和服务接口；在用户态实现机制和策略分离；建立隐私、能力、策略、认证和审计边界；在模拟器和 Pixel 6a 真机上跑通设备动作处理器与授权回路，并补齐 PreWarm、PrefetchFile、ReleaseMemory `cache:volatile` 的动作级收益证据。未深入的是：没有修改 scheduler、LMKD、VFS 或 Binder driver；普通 APK 受 Android 沙箱和后台启动限制；Binder/fanotify 探针仍依赖特权部署；系统级动作需要 platform signing 或 ROM 集成。
-->
---

# 下一步：从原型到系统研究

<div class="mt-5 grid grid-cols-3 gap-4 text-sm">

<div class="p-4 border rounded">

### 评测

- 构建真实用户、匿名化 ground truth 数据集
- Top-1 / Top-3、错误预热率
- 真机功耗和长期稳定性

</div>

<div class="p-4 border rounded">

### OS 集成

- 接入 LMKD / cgroup 反馈
- 使用 Binder 或 Unix domain socket
- platform-signed system app
- 验证 KeepAlive 的系统部署前提

</div>

<div class="p-4 border rounded">

### 决策

- 接入端侧轻量模型并与规则基线对照
- 在线反馈校准置信度
- 加入资源预算与预热撤销机制

</div>

</div>

<!--
（按评测、OS 集成、决策三类讲，每类一句。）
下一步分三类。评测上，需要构建真实用户、匿名化 ground truth 数据集，补真机功耗和长期稳定性，而不是只靠短窗口动作 gate。OS 集成上，可以接入 LMKD/cgroup 反馈，使用 Binder 或 Unix domain socket，做 platform-signed system app，尤其验证 KeepAlive 的系统部署前提。决策上，可以接入端侧轻量模型，与强预测基线继续对照，并加入资源预算和预热撤销机制。
-->
---

# 总结：DiPECS 是 OS control plane 原型

<div class="mt-7 grid grid-cols-3 gap-5 text-sm">
  <div class="p-5 border rounded"><div class="text-2xl mb-3">01</div><b>Syscall boundary</b><br/><br/>跨越信任边界的是 action schema 和 `AuthorizedAction`，不是原始模型文本。</div>
  <div class="p-5 border rounded"><div class="text-2xl mb-3">02</div><b>Scheduler</b><br/><br/>DecisionRouter + ActionBus + circuit breaker 调度本地、云端和 NoOp 路径。</div>
  <div class="p-5 border rounded"><div class="text-2xl mb-3">03</div><b>Audit</b><br/><br/>Golden trace 与 AuditRecord 复现状态迁移，而不是事后补日志。</div>
</div>

<div class="mt-8 p-5 border rounded text-center text-lg">
DiPECS 不是一个 app；它是面向 model-native systems 的用户态 OS 控制平面原型。
</div>

<!--
（回到六个关键词，最后“谢谢大家”后停顿，不要立刻切 Q&A。）
最后总结成三句话。第一，DiPECS 的 syscall boundary 是 action schema 和 AuthorizedAction，跨越信任边界的不是原始模型文本。第二，DecisionRouter、ActionBus 和 circuit breaker 共同承担 scheduler 角色，决定走本地、云端还是 NoOp。第三，Golden trace 和 AuditRecord 复现状态迁移，不是事后补日志。所以 DiPECS 不是一个 app，而是面向 model-native systems 的用户态 OS 控制平面原型。谢谢大家。
-->
