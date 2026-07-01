---
theme: touying
title: DiPECS — 面向 Android 本地上下文的智能决策与动作执行原型
info: |
  DiPECS final presentation
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
课程项目汇报 · 2026.07
</div>

---

# 目录

<div class="agenda-grid mt-7">

<div class="agenda-item">
  <div class="agenda-no">01</div>
  <div>
    <div class="agenda-title">项目动机与设计目标</div>
    <div class="agenda-desc">为什么需要本地上下文，以及为什么不能直接交给模型。</div>
  </div>
</div>

<div class="agenda-item">
  <div class="agenda-no">02</div>
  <div>
    <div class="agenda-title">系统架构与数据入口</div>
    <div class="agenda-desc">Android Collector、Rust ingress、daemon 运行时如何组成主链路。</div>
  </div>
</div>

<div class="agenda-item">
  <div class="agenda-no">03</div>
  <div>
    <div class="agenda-title">数据处理与信任边界</div>
    <div class="agenda-desc">RawEvent 如何变成 SanitizedEvent 和 StructuredContext。</div>
  </div>
</div>

<div class="agenda-item">
  <div class="agenda-no">04</div>
  <div>
    <div class="agenda-title">决策路由与授权动作</div>
    <div class="agenda-desc">模型只能提出建议，动作必须经过本地策略和生命周期。</div>
  </div>
</div>

<div class="agenda-item wide">
  <div class="agenda-no">05</div>
  <div>
    <div class="agenda-title">验证、演示路径与总结</div>
    <div class="agenda-desc">在线链路、离线 replay、audit hash 和最终展示口径。</div>
  </div>
</div>

</div>

---
layout: section
---

# 01 项目动机与设计目标

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

- 通知文本可能包含个人信息
- 应用行为可能反映隐私习惯
- 设备状态会影响动作是否合适
- 自动执行必须有明确边界

</div>

</div>

<div class="mt-8 p-4 border rounded text-sm">
DiPECS 的目标不是让模型“什么都能看、什么都能做”，而是在本地把信息压缩成可用但受控的上下文。
</div>

---

# 设计目标

<div class="mt-6 grid grid-cols-2 gap-4 text-sm">

<div class="p-4 border rounded">

### 上下文感知

把应用切换、通知、设备状态等信号统一成事件流。

</div>

<div class="p-4 border rounded">

### 本地优先处理

原始事件只在本地短路径内存在，模型只接收结构化上下文。

</div>

<div class="p-4 border rounded">

### 受控动作执行

决策模块只能提出建议，真正执行前必须经过本地策略检查。

</div>

<div class="p-4 border rounded">

### 可回放与可审计

同一段输入 trace 可以复现处理结果，动作路径可以生成 audit record。

</div>

</div>

---
layout: section
---

# 02 系统架构与数据入口

---

# 总体架构

<div class="diagram-placeholder">
  <div>
    <div class="placeholder-title">预留：总体架构图</div>
    <div class="placeholder-subtitle">Android Collector → aios-collector → aios-core → aios-agent → aios-action → Android Bridge / Replay Audit</div>
  </div>
</div>

<div class="mt-4 text-sm opacity-75">
主链路：采集 → 入口标准化 → 隐私处理 → 窗口聚合 → 决策 → 策略检查 → 授权执行。
</div>

---

# 模块职责

<div class="grid grid-cols-2 gap-4 mt-4 text-sm">

<div>

| 模块 | 作用 |
| --- | --- |
| `apps/android-collector` | 设备侧公开 API 采集 |
| `aios-collector` | Android JSONL tail 与内部事件入口 |
| `aios-core` | 隐私边界、窗口聚合、策略、动作生命周期 |
| `aios-agent` | 本地规则、可选云端 LLM、兜底路由 |

</div>

<div>

| 模块 | 作用 |
| --- | --- |
| `aios-action` | 默认本地 stub 与 Android bridge 转发 |
| `aios-daemon` | 长期运行管线，组装采集与处理任务 |
| `aios-cli` | replay、audit、Android socket 调试 |
| `aios-spec` | 跨模块数据协议和类型定义 |

</div>

</div>

<div class="mt-5 p-3 border rounded text-sm">
每个模块只拥有自己需要的能力，原始数据、决策建议和执行权限不会混在一起。
</div>

---

# OS 概念映射

<div class="mt-4 text-sm compact-table">

| OS 概念 | DiPECS 对应 |
| --- | --- |
| 系统调用 ABI | Action Schema（`aios-spec` 中的结构化动作类型） |
| 内核策略引擎 | `PolicyEngine`（风险、能力、置信度检查） |
| 设备驱动 | `ActionAdapter` trait（Offline / Android / Linux） |
| 进程追踪 (strace) | Golden Trace 确定性 replay |
| 安全边界 / 信任域 | `PrivacyAirGap`（RawEvent 不越过此边界） |
| Capability 权限系统 | `AuthorizedAction`（唯一构造点在 `ActionLifecycle`） |
| 审计日志 | `AuditRecord`（每个 action 产出一条终态记录） |
| 调度器 | `ActionBus` + `DecisionRouter`（mpsc channel + 优先级路由） |

</div>

<div class="mt-5 text-sm opacity-75">
DiPECS 是一个面向模型原生 OS 的用户态控制平面原型。模型的输出只是“意图提案”，必须经过本地的能力检查、策略裁决和调度后才能执行。
</div>

---

# 本地数据源

<div class="mt-4 text-sm compact-table">

| 来源 | 当前状态 | 进入 Rust 的事件 |
| --- | --- | --- |
| `UsageStatsManager` | 已接入 | `AppTransition` / `ScreenState` |
| `NotificationListenerService` | 已接入 | `NotificationPosted` / `NotificationInteraction` |
| Device context heartbeat | 已接入 | `SystemState` |
| AccessibilityService | 用于界面预览和筛选 | `rawEvent: null`，生产管线跳过 |
| `/proc` 差分 | daemon 已接入 | `ProcStateChange` |
| BinderProbe | 接口预留，当前 stub | `BinderTransaction` 预留 |
| fanotify / VFS | spec 预留 | `FileSystemAccess` 预留 |

</div>

<div class="mt-5 text-sm opacity-75">
当前优先使用用户可授权、可稳定复现的数据源；更底层的能力保留接口，不作为展示链路的前置依赖。
</div>

---

# 为什么暂缓 eBPF / 内核追踪

<div class="mt-5 text-sm leading-relaxed">

裸系统调用 trace 本身不携带用户意图语义。在内核 VFS 层拦截 `open()`、`read()`、`write()` 只能告诉你“发生了什么系统调用”，无法区分“用户打开了 PDF”和“缓存守护进程写盘”——没有上层信号融合，syscall trace 就是噪声。

<div class="mt-4 p-4 border rounded">

**OS 设计考量**：DiPECS 选择先接入携带**语义意图**的公开 API 数据源（应用切换、通知类型、设备状态），在内核层保留 Binder / eBPF 接口作为扩展点。这类似于内核中的 **ftrace → perf_event → eBPF** 演进路径——机制层先用最小侵入的方式接入，语义层逐步叠加。Binder / fanotify / eBPF 接口已在 spec 中预留，后续可在不改变上层管线的前提下接入真实内核事件。

</div>

</div>

---

# Android Collector

<div class="grid grid-cols-2 gap-5 mt-5 text-sm">

<div>

### 设备侧采集

- 前台应用变化
- 通知发布和交互
- 电量、网络、屏幕、铃声模式
- 可选无障碍事件预览

</div>

<div>

### 输出格式

- 写入 app 私有目录下的 `actions.jsonl`
- append-only，便于持续 tail
- 每行是一个 `CollectorEvent`
- 非空 `rawEvent` 才进入生产管线

</div>

</div>

```json
{
  "timestampMs": 1782854400000,
  "source": "notification",
  "rawEvent": { "NotificationPosted": { "...": "..." } }
}
```

---

# Rust 侧事件入口

<div class="diagram-placeholder">
  <div>
    <div class="placeholder-title">预留：Rust 侧事件入口图</div>
    <div class="placeholder-subtitle">actions.jsonl / AndroidJsonlTailer / RustCollectorIngress / CollectorEnvelope / PrivacyAirGap</div>
  </div>
</div>

<div class="mt-5 grid grid-cols-2 gap-4 text-sm">

<div class="p-3 border rounded">
文件只读取新增完整行；截断或轮转时重置 offset。
</div>

<div class="p-3 border rounded">
Android JSONL 标为 `PublicApi`，daemon 内部事件标为 `Daemon`。
</div>

</div>

---

# 运行时管线

<div class="diagram-placeholder">
  <div>
    <div class="placeholder-title">预留：运行时管线图</div>
    <div class="placeholder-subtitle">Collection Task / raw_events channel / Processing Task / RuntimeTraceRecorder</div>
  </div>
</div>

<div class="mt-4 text-sm opacity-75">
采集和处理解耦：采集侧持续产生事件，处理侧按窗口聚合后再触发决策和动作治理。
</div>

---

# 窗口聚合

<div class="grid grid-cols-2 gap-5 mt-5 text-sm">

<div>

### 为什么需要窗口

- 单个事件上下文不足
- 减少每条事件都触发决策的开销
- 对短时间内的行为形成摘要
- 让 replay 更容易对齐

</div>

<div>

### `StructuredContext`

- `foreground_apps`
- `notified_apps`
- `all_semantic_hints`
- `file_activity`
- `latest_system_status`
- `source_tier`

</div>

</div>

<div class="mt-6 p-4 border rounded text-sm">
`StructuredContext` 是决策后端可见的唯一上下文格式。
</div>

---
layout: section
---

# 03 数据处理与信任边界

---

# 隐私边界（安全边界 / 信任域）

<div class="grid grid-cols-2 gap-5 mt-5 text-sm">

<div class="diagram-placeholder compact-placeholder">
  <div>
    <div class="placeholder-title">预留：隐私边界图</div>
    <div class="placeholder-subtitle">RawEvent → PrivacyAirGap → SanitizedEvent → StructuredContext → Decision Backend</div>
  </div>
</div>

<div class="compact-table">

| 原始信息 | 处理后 |
| --- | --- |
| 通知标题 / 正文 | `TextHint` + `SemanticHint` |
| 文件路径 | 只保留扩展名类别 |
| Binder payload | 不保存 payload |
| Notification key | 丢弃，避免 tag 携带敏感信息 |

</div>

</div>

<div class="mt-5 p-3 border rounded text-sm">
类比 OS 中的用户态/内核态隔离：`RawEvent` 是“内核态的原始数据”，`PrivacyAirGap` 是唯一允许跨越信任边界的门（gate）。模型后端永远只接收脱敏后的 `StructuredContext`，类似于用户态进程只能通过系统调用访问内核资源，而不能直接读写内核内存。
</div>

---

# 从原始文本到语义提示

<div class="grid grid-cols-3 gap-4 mt-6 text-sm">

<div class="p-4 border rounded">

### `TextHint`

- 字符长度
- 书写系统
- 是否纯 emoji

</div>

<div class="p-4 border rounded">

### `SemanticHint`

- 文件
- 图片
- 语音
- 链接
- 日历
- 验证码

</div>

<div class="p-4 border rounded">

### 本地完成

- 关键词匹配在设备侧完成
- 不上传原始文本
- 缺失信息显式表达为 `None`

</div>

</div>

<div class="mt-7 text-sm opacity-75">
模型看到的是“这是一条可能与文件相关的通知”，而不是通知原文。
</div>

---
layout: section
---

# 04 决策路由与授权动作

---

# 决策路由（类比 OS 调度器）

<div class="diagram-placeholder">
  <div>
    <div class="placeholder-title">预留：决策路由图</div>
    <div class="placeholder-subtitle">StructuredContext / DecisionRouter / RuleBasedBackend / CloudLlmBackend / FallbackNoOpBackend / IntentBatch</div>
  </div>
</div>

<div class="grid grid-cols-2 gap-4 mt-5 text-sm">

<div class="p-3 border rounded">

### 路由优先级（类比调度策略）

1. **熔断器触发** → 直接兜底（过载保护）
2. **隐私敏感度 > 阈值** → 本地规则（安全优先）
3. **本地可行动信号** → 本地评估器（快速路径）
4. **语义复杂度** → 云端 LLM（复杂场景）

</div>

<div class="p-3 border rounded">

### OS 设计考量

- **电路熔断器**：连续错误后自动降级，类似于内核的 watchdog 机制
- **多级 fallback**：云端失败 → 规则兜底；熔断 → 纯 NoOp
- **延迟分级**：微秒级（本地规则）、亚毫秒级（本地评估）、秒级（云端 LLM）
- 与 OS 中断合并类似——窗口聚合将高频事件批处理，减少调度开销

</div>

</div>

<div class="mt-5 text-sm">

当前默认路径优先使用本地规则。云端后端只有在环境变量启用且配置完整时参与；失败时回落到本地规则，连续错误后进入兜底。

</div>

---

# 模型只能提出建议（类比系统调用边界）

<div class="diagram-placeholder">
  <div>
    <div class="placeholder-title">预留：动作治理链路图</div>
    <div class="placeholder-subtitle">IntentBatch → SuggestedAction → ActionProposal → PolicyEngine → AuthorizedAction → ActionAdapter → AuditRecord</div>
  </div>
</div>

<div class="mt-5 p-4 border rounded text-sm">
类比 OS 系统调用：模型（用户态）只能提出 `IntentBatch` 建议，真正可执行的 `AuthorizedAction` 必须经过内核态的 `ActionLifecycle` + `PolicyEngine` 校验后才能构造——执行器不能自行伪造授权动作。这与用户态进程无法绕过 syscall gate 直接操作内核资源同理。
</div>

---

# 策略检查（类比内核 Capability 检查）

<div class="grid grid-cols-2 gap-5 mt-5 text-sm">

<div>

### 检查项（类比 capability 校验）

- 后端能力等级 → 类似 capability bound
- 风险等级上限 → 类似 seccomp filter
- 置信度下限
- blocked action 子串
- `Deferred` urgency 拒绝
- 单 intent action 数量上限
- target 是否出现在当前上下文

</div>

<div>

### 拒绝原因

- `RiskExceedsCapability`
- `ActionCapabilityDenied`
- `TargetNotInContext`
- `ConfidenceTooLow`
- `BlockedAction`
- `DeferredUrgency`

</div>

</div>

<div class="mt-6 text-sm opacity-75">
类比 Linux capability / seccomp：`PolicyEngine` 定义了动作允许的能力上界。每个后端路由对应一个 `CapabilityLevel`（Low / Medium），超出能力范围的建议动作会被拒绝。这与内核中 `CAP_SYS_ADMIN` 等能力检查的设计理念一致——最小权限原则。
</div>

---

# 动作生命周期

<div class="diagram-placeholder">
  <div>
    <div class="placeholder-title">预留：动作生命周期状态机</div>
    <div class="placeholder-subtitle">Proposed / SchemaValidated / PolicyChecked / Dispatched / Succeeded / Failed / Denied</div>
  </div>
</div>

<div class="mt-5 text-sm">
每个 `(window_ordinal, intent_ordinal, action_ordinal)` 形成确定性坐标，并产出一条终态 `AuditRecord`。
</div>

---

# Android 动作桥

<div class="grid grid-cols-2 gap-5 mt-5 text-sm">

<div>

### 通信方式

- localhost socket
- token 认证
- freshness check
- action signature
- debug / release token 策略分离

</div>

<div>

### 动作集合

- `PrefetchFile`
- `ReleaseMemory`
- `KeepAlive`
- `PreWarmProcess`
- release 入口只接受受控 payload

</div>

</div>

<div class="mt-6 p-4 border rounded text-sm">
Android 侧不是开放任意执行入口，而是接收经过本地生命周期和策略检查后的有限动作。这类似于 OS 内核只允许通过系统调用接口访问受限资源。
</div>

---

# 资源管理（用户态资源调度器）

<div class="grid grid-cols-2 gap-5 mt-5 text-sm">

<div>

### 读取内核级状态

- `/proc` 差分：PSS、RSS 进程内存快照
- `SystemState`：电量、网络、屏幕状态
- daemon 通过 tokio 异步采集，不阻塞主窗口

</div>

<div>

### 执行资源调度动作

| 动作 | OS 类比 |
| --- | --- |
| `PreWarmProcess` | 进程预加载（类 sched 提示） |
| `ReleaseMemory` | 内存回收（类 LMK / cgroup 调整） |
| `PrefetchFile` | 文件预取（类 readahead） |
| `KeepAlive` | 进程保活（类 oom_score_adj） |

</div>

</div>

<div class="mt-6 p-4 border rounded text-sm">
DiPECS 作为用户态资源管理器：读取内核级状态，应用模型生成的预测作为调度提示，最终通过 Android OS 服务执行。内核保留最终裁决权——这与 `sched_ext` 等可扩展调度器的设计理念一致。
</div>

---

# 回放与审计

<div class="diagram-placeholder">
  <div>
    <div class="placeholder-title">预留：回放与审计图</div>
    <div class="placeholder-subtitle">CollectorEvent JSONL / aios-cli replay / OfflineAdapter / canonical audit stream / audit_hash</div>
  </div>
</div>

<div class="mt-5 grid grid-cols-2 gap-4 text-sm">

<div class="p-3 border rounded">
Replay 不访问真实设备、网络或 Android，只返回确定性结果。
</div>

<div class="p-3 border rounded">
Audit hash 会剥离 UUID、latency 等不稳定字段，用于回归验证。
</div>

</div>

---
layout: section
---

# 05 验证、演示路径与总结

---

# 测试与验证

<div class="mt-4 text-sm compact-table">

| 区域 | 覆盖内容 |
| --- | --- |
| `aios-spec` | 数据协议 serde、边界契约 |
| `aios-core` | 隐私泄漏、窗口聚合、策略、动作生命周期 |
| `aios-collector` | Android ingress、collection stats |
| `aios-cli` | replay、audit hash、Android adapter |
| Android app | event store、raw event mapper、action bridge |
| Emulator flow | APK 安装、端口转发、socket health check |

</div>

<div class="mt-6 p-4 border rounded text-sm">
展示版本按完整链路准备：Android 采集、Rust 入口、脱敏聚合、决策、策略、动作桥、replay/audit 均可演示。
</div>

---

# 演示路径

<div class="mt-5 text-sm">

1. Android Collector 采集应用切换、通知和设备状态
2. `actions.jsonl` 作为 append-only 输入被 Rust tail
3. `RawEvent` 进入 `PrivacyAirGap`
4. 窗口关闭后生成 `StructuredContext`
5. `DecisionRouter` 生成 `IntentBatch`
6. `PolicyEngine` 与 `ActionLifecycle` 生成终态审计
7. Android bridge 接收受控动作
8. replay 使用同一份 trace 复现处理结果

</div>

<div class="mt-8 text-lg">
同一条链路既能在线运行，也能离线验证。
</div>

---

# 当前完成度

<div class="grid grid-cols-2 gap-5 mt-5 text-sm">

<div>

### 已完成

- Android promoted sources 采集
- Android JSONL production ingress
- daemon 运行时管线
- `PrivacyAirGap` 与窗口聚合
- 本地规则与可选云端决策路由
- `PolicyEngine` 与 `ActionLifecycle`
- Android action bridge
- replay、audit hash、golden 测试

</div>

<div>

### 展示口径

- 主链路端到端打通
- 原始事件不会直接进入决策后端
- 模型建议不能绕过策略直接执行
- 在线路径和离线路径复用同一套核心逻辑
- 权限更高的数据源作为扩展能力保留

</div>

</div>

---

# 设计取舍

<div class="grid grid-cols-2 gap-5 mt-5 text-sm">

<div class="p-4 border rounded">

### 先做公开 API

优先保证普通设备上可部署、可授权、可复现；底层内核接口保留但不强依赖（机制与策略分离——机制层独立于数据源）。

</div>

<div class="p-4 border rounded">

### 先做本地规则

默认不依赖云端；云端能力通过配置启用，并且失败时可以降级。

</div>

<div class="p-4 border rounded">

### 先做审计闭环

动作成功不只是“执行了”，还必须能解释来源、状态迁移和终态。

</div>

<div class="p-4 border rounded">

### 先做有限动作

动作集合保持保守，降低自动执行的风险面（类比最小 capability 原则——只授予必要的动作类型）。

</div>

</div>

---

# 项目总结

<div class="mt-4 text-left text-lg leading-9">

**DiPECS 是一个面向模型原生 OS 的用户态控制平面原型。**

<br/>

- **系统调用边界**：Action Schema 定义动作接口，`PolicyEngine` + `ActionLifecycle` 构成 syscall gate——模型意图必须经过本地能力检查后才能执行
- **调度器**：`DecisionRouter` 按熔断器 → 隐私敏感度 → 可行动信号 → 语义复杂度四级路由，融合电路熔断器保护
- **审计闭环**：Golden Trace 确定性 replay + `audit_hash`，每次 `cargo test` 验证回归——动作不只是“执行了”，而是“可复现、可验证的 OS 状态迁移”
- **安全边界**：`PrivacyAirGap` 确保原始事件不越过信任域；模型只看脱敏后的结构化上下文

<br/>

项目的重点不是让模型拥有更多权限，而是在真实设备边界内，把观察、决策和执行组织成可验证的 OS 级控制回路。

</div>

---

# Q&A

<div class="mt-12 text-xl opacity-70">
谢谢
</div>
