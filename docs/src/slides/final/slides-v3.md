---
theme: touying
title: DiPECS — 面向 Android/Linux 的本地优先 AIOS 原型系统
info: |
  DiPECS final presentation v3 · 40 minutes
layout: cover
class: text-center
transition: slide-left
duration: 40min
drawings:
  persist: false
mdc: true
touying:
  preset: simple
  footer: DiPECS · Local-First Android/Linux AIOS
---

# DiPECS

## 面向 Android/Linux 的本地优先 AIOS 原型系统

<div class="mt-7 text-lg opacity-80">
感知 · 脱敏 · 决策 · 授权执行 · 回放审计
</div>

<!--
（封面页：先停 1 秒，面向老师。不要读副标题太快。）
各位老师、同学大家好，我们汇报的项目是 DiPECS。我们把它定位为一个面向 Android 和 Linux 的本地优先 AIOS 原型系统。今天汇报的核心不是某一个模型效果，而是一条完整的 OS 控制闭环：本地信号怎样进入系统，隐私怎样被隔离，模型建议怎样经过授权，动作怎样执行，最后怎样审计和回放。
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
探索智能操作系统中<strong>感知—决策—授权执行—审计</strong>的本地可信闭环
</div>

<!--
（手指从左到右扫过“感知、脱敏、上下文、决策、授权执行、审计”。这里慢一点。）
DiPECS 可以概括为六个环节：感知、脱敏、上下文、决策、授权执行、审计。系统从应用切换、通知、设备状态、进程状态等本地信号采集事件；通过 Privacy Air Gap 去掉原始文本、路径、标识符；再聚合成上下文窗口。规则、本地评估器或可选云模型只生成候选意图，真正动作必须经过 PolicyEngine 和 ActionLifecycle，形成 AuthorizedAction 后才执行。每个动作都会留下终态审计记录，离线 replay 使用同一套核心逻辑复现。
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

# 从“AI 能力”到“可信 AIOS 闭环”

<div class="grid grid-cols-2 gap-6 mt-6 text-sm">

<div class="p-5 border rounded">

### 系统侧问题

感知信号 → 模型建议 → 系统动作

- 本地信号分散在不同 OS 接口
- 原始通知与行为数据具有隐私风险
- 模型输出不具备系统执行权限
- 网络与模型失败不能阻塞控制路径

</div>

<div class="p-5 border rounded">

### DiPECS 的研究问题

能否把 AI 建议约束在可控的 OS 机制内？

- 本地优先完成脱敏、决策与降级
- 模型只产生 Intent，不直接操作系统
- 动作必须经过授权、执行和终态审计
- 预测错误时允许拒绝和 `NoOp`

</div>

</div>

<div class="mt-6 p-4 border rounded text-center">
核心不是“让模型控制 OS”，而是让模型在<strong>权限、策略与审计边界</strong>内参与系统决策。
</div>

<!--
（先指左侧“系统侧问题”，再指右侧“研究问题”，最后指底部核心句。）
现在大模型越来越多地进入应用和系统服务，可以总结通知、理解意图、预测下一步操作。但操作系统层面关心的是另一组问题：本地信号分散在不同 OS 接口，原始通知和行为数据有隐私风险，模型输出本身没有系统执行权限，网络和模型失败也不能阻塞关键控制路径。
DiPECS 的研究问题是：能否把 AI 建议约束在可控的 OS 机制内。也就是说，AI 可以参与决策，但必须在权限、策略和审计边界内参与。
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
项目目标有五个：统一 Android 和 Linux 信号，强制隔离原始隐私，多级决策可降级，动作经过策略、认证和审计，决策结果可评测。
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
  <img src="/diagrams/1module-deps.svg" />
</div>

<div class="diagram-caption">
`aios-spec` 固化跨模块类型、trait 与 IPC 协议；daemon / CLI 只是组合入口，不反向污染核心库。
</div>

<!--
（先指顶部 `aios-spec`，再指四个核心库，最后指 daemon/CLI。）
模块依赖上，我们把稳定协议放在 `aios-spec`。它定义跨模块类型、trait 和 IPC 协议。`aios-collector`、`aios-core`、`aios-agent`、`aios-action` 依赖 `aios-spec`，daemon 和 CLI 只是组合入口，不反向污染核心库。这体现机制和策略分离：采集器和执行器提供机制，Router 和 Policy 决定策略。
-->
---

# OS 相关性：接口、问题、原则

<div class="mt-4 compact-table text-sm">

| 层次 | DiPECS 中的实现 | OS 课程关联 |
|---|---|---|
| 进程 | `/proc` 快照差分、`fork + setsid`、signal | 进程状态、daemon、生命周期 |
| IPC | localhost socket、请求/响应、超时 | 进程间通信、阻塞与失败语义 |
| 文件 | append-only JSONL、positional tail、flush | 文件偏移、追加语义、持久化 |
| 资源 | PreWarm / KeepAlive / Prefetch / Release | 进程、内存、I/O 资源管理 |
| 安全 | capability、policy、HMAC、审计 | 权限分离、最小权限、引用监控器 |
| Android | UsageStats、通知服务、JobScheduler | OS 服务与受控系统接口 |

</div>

<div class="mt-5 p-4 border rounded text-center text-sm">
定位：<strong>本地优先的用户态 AIOS 原型</strong>，以 OS 机制约束智能决策，而不是修改内核算法。
</div>

<!--
（按表格从上到下扫一遍，重点强调课程相关性。）
从 OS 课程角度看，这个项目不是单纯应用开发，而是围绕进程、文件、IPC、权限和审计做用户态控制平面。daemon 对应进程与会话管理，`/proc` 对应内核暴露的进程状态接口，JSONL tail 对应文件系统增量读取，mpsc channel 对应任务解耦，Policy/HMAC/审计对应最小权限和引用监控器式边界。
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
  <img src="/diagrams/1runtime-pipeline.svg" />
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

# 10 秒上下文窗口

<div class="diagram-frame">
  <img src="/diagrams/window-aggregation.svg" />
</div>

<div class="diagram-caption">
窗口结束后才触发一次决策；避免为每个噪声事件调用模型。
</div>

<!--
（手指沿时间轴移动，强调多个事件进入同一个窗口。）
系统不会为每个噪声事件单独触发模型，而是在 10 秒窗口内聚合应用切换、通知、系统状态、进程状态、文件活动等事件。窗口关闭后形成 StructuredContext，包含起止时间、事件列表、摘要和行为特征。这样既减少模型调用频率，也让策略可以判断动作目标是否真的出现在当前上下文里。
-->
---

# DecisionRouter：分级路由与熔断

<div class="diagram-frame">
  <img src="/diagrams/decision-routing.svg" />
</div>

<div class="diagram-caption">
路由优先级：熔断状态 → 隐私敏感度 → 本地可动作信号 → 语义复杂度。连续错误按时间窗口计数，成功后清零。
</div>

<!--
（按优先级从上到下讲：熔断、隐私、本地信号、语义复杂度。）
DecisionRouter 的优先级是：先看熔断状态，如果最近连续错误超过阈值，直接 FallbackNoOp；再看隐私敏感度，如果验证码或金融语义太多，阻止云端路径；再看本地可动作信号，比如文件访问、低电量、屏幕交互，这些直接走本地评估；最后才根据语义复杂度选择 RuleBased、LocalEvaluator 或 Cloud LLM。云端配置错误或调用失败时会退回本地规则，并记录错误。
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

# PolicyEngine：模型建议需经授权

<div class="diagram-frame">
  <img src="/diagrams/policy-check-flow.svg" />
</div>

<div class="diagram-caption">
八项检查按顺序 fail closed；只有全部通过才产生 `PolicyActionDecision::Approved`。
</div>

<!--
（逐项指 8 个检查框，不要展开每个实现细节；重点讲顺序执行和 fail closed。）
PolicyEngine 是模型建议到系统动作之间的硬边界。它按顺序做 8 项检查：后端能力等级、风险配置、置信度、batch 上限、阻止列表、延迟动作紧急度、动作能力白名单，以及目标是否在上下文中。任一失败都会拒绝，并写入 DenialReason。只有全部通过，才产生 `PolicyActionDecision::Approved`。
-->
---

# ActionLifecycle：每个动作恰好一个终态

<div class="diagram-frame">
  <img src="/diagrams/action-lifecycle.svg" />
</div>

<div class="diagram-caption">
只有 lifecycle 能构造 `AuthorizedAction`；连接、超时、设备拒绝与非法回执均进入可审计终态。
</div>

<!--
（沿状态机走：校验、Policy、seal、adapter、终态。）
ActionLifecycle 保证每个动作恰好一个终态。它先做 schema 校验，再调用 PolicyEngine，再 seal AuthorizedAction，再调用 adapter。如果 adapter 成功，记录 Succeeded；连接失败、超时、设备拒绝、非法回执都会进入 Failed；策略拒绝会进入 DeniedByPolicy 或相关终态。只有 lifecycle 能构造 AuthorizedAction，执行层不能自己伪造。
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
| `PreWarmProcess` | 启动目标 Activity，轮询任务后清理可见 task | 后台启动第三方应用受限；系统签名更完整 |
| `PrefetchFile` | HTTPS / URI 预取到受控缓存 | 普通 APK 可执行自身可访问资源 |
| `KeepAlive` | OOM/cgroup 尝试 + JobScheduler fallback | OOM/cgroup 写入通常需要特权 |
| `ReleaseMemory` | 清理预取缓存；尝试包缓存与 page cache | 跨应用/全局回收需要系统权限 |
| `NoOp` | 确定性本地终态 | 安全退化路径 |

</div>

<div class="mt-5 p-4 border rounded text-sm">
同一动作在普通 APK 环境可能退化、被拒绝或只作用于自身资源；这属于 Android 权限模型，而不是隐藏为“成功”。
</div>

<!--
（依次点 PreWarm、Prefetch、KeepAlive、ReleaseMemory、NoOp。不要承诺收益。）
动作边界要讲清楚。`PreWarmProcess` 可以启动目标 Activity，但普通 APK 对第三方后台启动会受限；`PrefetchFile` 可以预取 HTTPS 或 content URI 到受控缓存；`KeepAlive` 尝试 OOM/cgroup 并 fallback 到 JobScheduler；`ReleaseMemory` 清理预取缓存，特权环境下可以尝试包缓存或 page cache；`NoOp` 是确定性安全退化路径。不同权限环境下的退化、拒绝或只作用于自身资源，都会在审计中明确体现。
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

# 设备执行证据：四类处理器均有终态

<div class="mt-4 compact-table text-sm">

| 动作 | 设备终态审计事件 | 确认延迟 | 结果 |
|---|---|---:|---|
| KeepAlive | `keep_alive_scheduled → job_executed` | 21.3 ms | EXECUTED |
| ReleaseMemory | `release_memory_completed` | 13.4 ms | EXECUTED |
| PreWarmProcess | `own_resources_prewarmed` | 31.2 ms | EXECUTED* |
| PrefetchFile | `prefetch_started → prefetch_succeeded` | 1.1 ms* | EXECUTED |

</div>

<div class="mt-5 grid grid-cols-2 gap-4 text-xs">
  <div class="p-3 border rounded">设备记录 `authorized_action_socket_execute_ok = 4`，四类处理器均留下终态。</div>
  <div class="p-3 border rounded">* PreWarm 验证 `own:warmup` 处理器链路，不证明第三方应用预热收益；Prefetch 回执仅表示入队，最终下载由 `prefetch_succeeded` 取证。</div>
</div>

<div class="mt-5 text-xs opacity-70">
诚实边界：设备证据经与生产信封逐字节一致的取证发送器通过 adb forward 获得；它证明设备处理器执行，不等价于 daemon 已在设备内生产部署。Rust `AndroidAdapter` 另有 mock-socket E2E。
</div>

<!--
（指四类处理器终态，快速扫，不逐条读日志。）
第三是设备执行证据。我们验证了四类可转发动作处理器都有终态：KeepAlive 记录 `keep_alive_scheduled` 到 `job_executed`；ReleaseMemory 记录 `release_memory_completed`；PreWarmProcess 记录 `own_resources_prewarmed`；PrefetchFile 记录 `prefetch_started` 到 `prefetch_succeeded`。设备侧记录 `authorized_action_socket_execute_ok = 4`。这里要诚实说明：这证明设备处理器执行，不等价于完整生产部署。
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
| RQ5 | 常驻开销是否可控？ | emulator CPU/RSS/PSS + replay 吞吐 |
| RQ6 | 动作治理是否覆盖主要拒绝路径？ | Policy 20 项测试 + action audit |

</div>

<div class="mt-5 text-xs opacity-75">
证据标签：真实 API、模拟器实测、离线 replay、估算值分别标注，不混为同一强度结论。
</div>

<!--
（先指问题，再指证据类型。）
我们围绕六个问题组织实验：管线能否从 Android 事件走到可复现审计；隐私边界是否阻止原始通知泄漏；本地决策为何适合即时路径；当前决策器能否预测上下文支持的下一应用；常驻开销是否可控；动作治理是否覆盖主要拒绝路径。每个问题后面对应不同强度的证据，避免把 smoke E2E、合成数据和真机结论混在一起。
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

# 负面结果与有效性威胁

<div class="mt-5 grid grid-cols-2 gap-5 text-sm">

<div class="p-4 border rounded">

### 当前数据不支持

- “预热快 43.8%”的因果结论：脚本混入 cold / warm process 差异
- ReleaseMemory 有效降低 PSS：结果反而增加 0.331 MB
- 模拟器电池/温度代表真机功耗
- Cloud LLM 能稳定产生有效即时动作
- 合成准确率代表真实用户行为

</div>

<div class="p-4 border rounded">

### 因此我们的表述

- 启动时间数据不进入当前正面结论，待独立 controller 重测
- ReleaseMemory 仅视为链路覆盖，不视为收益证明
- 功耗结果标为 estimated
- 云端视为非即时可选后端
- 预测结果同时报告 eligible coverage 与错误预测率

</div>

</div>

<!--
（主动讲，语气平稳。按四类负面结果依次点。）
负面结果必须单独说明。当前数据不支持“预热快 43.8%”的因果结论，因为脚本混入 cold/warm process 差异；不支持 ReleaseMemory 有效降低 PSS，结果反而增加 0.331 MB；不支持模拟器电池和温度代表真机功耗；不支持 Cloud LLM 能稳定产生有效即时动作；也不支持合成准确率代表真实用户行为。因此启动时间数据不进入正面结论，ReleaseMemory 只视为链路覆盖，功耗结果标为 estimated，云端视为非即时可选后端。
-->
---
layout: section
---

# 06 局限、未来工作与总结

<!--
（章节页：开始收束，不再引入新概念。）
最后一部分总结项目边界和下一步工作。
-->
---

# 项目边界：为什么是“用户态控制平面”

<div class="mt-5 grid grid-cols-2 gap-6 text-sm">

<div class="p-4 border rounded">

### 已实现

- 使用 OS 暴露的进程、文件、IPC 和服务接口
- 在用户态实现机制/策略分离
- 建立隐私、能力、策略、认证和审计边界
- 在模拟器跑通设备动作处理器与授权回路

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
DiPECS 当前是用户态控制平面。已实现的是：使用 OS 暴露的进程、文件、IPC 和服务接口；在用户态实现机制和策略分离；建立隐私、能力、策略、认证和审计边界；在模拟器跑通设备动作处理器与授权回路。未深入的是：没有修改 scheduler、LMKD、VFS 或 Binder driver；普通 APK 受 Android 沙箱和后台启动限制；Binder/fanotify 探针仍依赖特权部署；系统级动作需要 platform signing 或 ROM 集成。
-->
---

# 下一步：从原型到系统研究

<div class="mt-5 grid grid-cols-3 gap-4 text-sm">

<div class="p-4 border rounded">

### 评测

- 构建真实用户、匿名化 ground truth 数据集
- Top-1 / Top-3、错误预热率
- 用独立 controller 重做启动实验
- 真机功耗和长期稳定性

</div>

<div class="p-4 border rounded">

### OS 集成

- 接入 LMKD / cgroup 反馈
- 使用 Binder 或 Unix domain socket
- platform-signed system app

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
下一步分三类。评测上，需要构建真实用户、匿名化 ground truth 数据集，重做启动实验，测真机功耗和长期稳定性。OS 集成上，可以接入 LMKD/cgroup 反馈，使用 Binder 或 Unix domain socket，做 platform-signed system app。决策上，可以接入端侧轻量模型，与规则基线对照，并加入资源预算和预热撤销机制。
-->
---

# 总结

<div class="mt-7 grid grid-cols-3 gap-5 text-sm">
  <div class="p-5 border rounded"><div class="text-2xl mb-3">01</div><b>本地 AIOS 闭环</b><br/><br/>Android / Linux 信号进入统一管线，并形成可回放审计。</div>
  <div class="p-5 border rounded"><div class="text-2xl mb-3">02</div><b>授权而非直控</b><br/><br/>模型不接触原始隐私，也不能绕过本地策略直接执行。</div>
  <div class="p-5 border rounded"><div class="text-2xl mb-3">03</div><b>可评测且诚实</b><br/><br/>报告合成预测、动作终态和开销，同时公开覆盖限制与负面结果。</div>
</div>

<div class="mt-8 p-5 border rounded text-center text-lg">
DiPECS 为 AIOS 提供一种机制策略分离、可降级、可回放、可审计的用户态实践方案。
</div>

<!--
（回到六个关键词，最后“谢谢大家”后停顿，不要立刻切 Q&A。）
最后总结：DiPECS 探索的是智能操作系统中“本地感知、隐私边界、决策路由、授权执行、可回放审计”的闭环机制。它把模型建议放进 OS 熟悉的权限、策略、生命周期和审计框架里。这个原型说明，AIOS 的关键不只在于 AI 能力，还在于系统如何限制、验证和追责这些能力。谢谢大家。
-->
