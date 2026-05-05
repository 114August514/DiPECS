#import "@preview/touying:0.6.1": *
#import themes.university: *
#import "@preview/numbly:0.1.0": numbly

#show: university-theme.with(
  aspect-ratio: "16-9",
  config-info(
    title: [DiPECS 中期报告],
    subtitle: [面向 Android 的用户意图预测系统],
    author: [DiPECS Team],
    date: datetime.today(),
    institution: [USTC],
    logo: box(height: 1em, image("template/USTC-logo.png", height: 100%)),
  ),
)

#set text(lang: "zh")
#set heading(numbering: numbly("{1}.", default: "1.1"))

#title-slide()

// ══════════════════════════════════════════════════════════════
// PART 1: WHAT
// ══════════════════════════════════════════════════════════════
= What: 我们在做什么

== 项目定位

#block(fill: blue.lighten(85%), inset: 0.8em, radius: 4pt)[
  *DiPECS* 是一个面向 Android 的*云端 LLM 驱动的分布式意图操作系统原型*。

  #v(0.4em)
  核心命题：#h(0.3em)*在用户动作发生之前，预测其意图并主动调度系统资源*。
]

#v(0.4em)

#table(
  columns: (auto, 1fr),
  stroke: none,
  inset: (x: 0.4em, y: 0.4em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*目标平台*], [Android 13 (API 33)，AOSP 可定制环境],
  [*实现语言*], [Rust 1.95.0（核心逻辑）+ Kotlin（Android 采集/执行层）],
  [*关键能力*], [本地采集与脱敏、云端语义预测、本地资源优化执行],
)

== 系统闭环

#block(fill: luma(242), inset: 0.8em, radius: 4pt)[
  *本地采集* $arrow.r$ *隐私脱敏* $arrow.r$ *云端预测* $arrow.r$ *本地优化执行*
]

#v(0.4em)

#table(
  columns: (0.45fr, 1.15fr),
  stroke: none,
  inset: (x: 0.35em, y: 0.35em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*本地采集*], [`UsageStats`、通知、传感器、网络、电量与内核事件],
  [*隐私脱敏*], [位置、通知、文本等敏感信息在本地转换为粗粒度标签],
  [*云端预测*], [LLM 基于结构化上下文输出意图、置信度与风险等级],
  [*本地执行*], [只执行低风险优化：应用预热、缓存提示、进程保活],
)

== 为什么选择 Android 13

#block(fill: yellow.lighten(70%), inset: 0.8em, radius: 4pt)[
  选型原则：*稳定性优先，接口可控，便于从 App 原型逐步下沉到系统层*。
]

#v(0.4em)

#table(
  columns: (auto, 1fr),
  stroke: none,
  inset: (x: 0.35em, y: 0.35em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*API 完备*], [`UsageStatsManager`、`NotificationListenerService`、`AppStandbyBucket` 等能力成熟],
  [*AOSP 成熟*], [Android 13 经历完整 QPR 周期，系统服务接口稳定],
  [*工具链可靠*], [NDK r27d 与 Rust 交叉编译链路可验证],
  [*内核可扩展*], [Android 12+ 已支持 eBPF 基础能力，便于采集系统级事件],
)

== 架构分层

#block(fill: luma(242), inset: 0.8em, radius: 4pt)[
  采用严格单向依赖流，由 CI 检查依赖边界，避免上层业务逻辑反向污染底层契约。
]

#v(0.3em)

#table(
  columns: (auto, auto, 1fr),
  stroke: none,
  inset: (x: 0.28em, y: 0.32em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*层级*], [*Crate*], [*职责*],
  [体验层], [`aios-cli`], [CLI/TUI 沙盒与观测工具],
  [业务层], [`aios-agent`], [LLM 代理、语义分解、上下文管理],
  [逻辑层], [`aios-core`], [Action Bus、意图调度、隐私 air-gap],
  [内核层], [`aios-kernel`], [资源生命周期、IPC、进程协调],
  [适配层], [`aios-adapter`], [Android Binder、Linux syscalls、离线回放],
  [宪法层], [`aios-spec`], [共享类型、特征与 schema 的唯一真相源],
)

== 机制-策略分离

#table(
  columns: (0.36fr, 0.62fr, 0.82fr),
  stroke: none,
  inset: (x: 0.3em, y: 0.35em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [], [*云端：Policy*], [*本地：Mechanism*],
  [*性质*], [非确定性推理], [确定性执行],
  [*职责*], [意图理解、模式推断、Skills 编排], [脱敏、动作路由、权限与风险校验],
  [*状态*], [无状态推理], [可观测、可回放的状态机],
  [*安全*], [只输出建议], [保留最终执行权],
)

#v(0.4em)

#block(fill: blue.lighten(85%), inset: 0.7em, radius: 4pt)[
  LLM 输出 `Plan / Parameter / Confidence / Risk`，本地 Policy Engine 决定是否执行。模型参与规划，*不直接控制设备*。
]

// ══════════════════════════════════════════════════════════════
// PART 2: WHY
// ══════════════════════════════════════════════════════════════
= Why: 为什么需要这个系统

== 移动端体验痛点

#block(fill: red.lighten(80%), inset: 0.8em, radius: 4pt)[
  *冷启动延迟* 是移动端体验中的高频痛点：应用切换越频繁，Dex 加载、资源解析、数据库初始化和后台重建越容易被用户感知。
]

#v(0.4em)

#table(
  columns: (0.5fr, 0.8fr),
  stroke: none,
  inset: (x: 0.35em, y: 0.35em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*传统系统*], [内存压力出现后才回收或调度，主要是被动响应],
  [*DiPECS*], [预测用户下一步应用，提前预热关键资源，将冷启动转为热启动],
  [*预期收益*], [在高置信度场景中减少 100--200ms 甚至数秒级等待],
)

== 语义鸿沟

#block(fill: luma(242), inset: 0.8em, radius: 4pt)[
  Android 能提供大量遥测，但系统知道的是 *What / When*，不知道用户意图层面的 *Why / What-Inside*。
]

#v(0.3em)

#table(
  columns: (0.5fr, 0.5fr),
  stroke: none,
  inset: (x: 0.28em, y: 0.28em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*低语义事件*], [*需要补足的高语义信息*],
  [包名、时间戳、前台时长], [用户为什么切换应用],
  [通知 channel、PID、UID], [跨应用任务链和注意力需求],
  [内存、网络、I/O 消耗], [是否值得保留或预加载资源],
)

#v(0.3em)

#block(fill: blue.lighten(85%), inset: 0.65em, radius: 4pt)[
  DiPECS 的定位是在 PID 级原始事件与意图级语义之间架桥。
]

// ══════════════════════════════════════════════════════════════
// PART 3: HOW
// ══════════════════════════════════════════════════════════════
= How: 我们如何实现

== Android 侧信息来源

#block(fill: luma(242), inset: 0.75em, radius: 4pt)[
  信息采集分为公开 API、系统特权 API、内核事件三层；普通 APK 可验证算法，系统镜像可验证真实资源收益。
]

#v(0.3em)

#table(
  columns: (auto, 1fr),
  stroke: none,
  inset: (x: 0.24em, y: 0.26em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [`UsageStatsManager`], [前后台切换、累计使用时长、事件流与 Standby Bucket],
  [`NotificationListener`], [通知来源、channel、出现/撤销时间],
  [`NetworkStats` / `Battery`], [按 UID 聚合网络、电量和唤醒行为],
  [`Sensor` / `Location`], [运动状态与粗粒度地点上下文],
  [`PreloadManager`], [系统级预加载回调和预取任务提示],
)

== 内核级信息抽取

#block(fill: red.lighten(80%), inset: 0.75em, radius: 4pt)[
  仅靠应用层 API 无法刻画真实资源成本，因此需要 eBPF 或自定义内核模块补足系统级事件。
]

#v(0.3em)

#table(
  columns: (auto, 1fr),
  stroke: none,
  inset: (x: 0.28em, y: 0.32em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*进程生命周期*], [`sched_process_fork` / `sched_process_exit`，获得精确启动与退出时间],
  [*文件 I/O*], [`openat` / `read` trace，构建冷启动热点文件画像],
  [*内存压力*], [`mm_page_alloc` / `vmscan` trace，观察 footprint 与回收事件],
  [*Binder 调用*], [`binder_transaction` trace，识别跨进程任务链],
)

== 多源信息融合

#block(fill: yellow.lighten(70%), inset: 0.8em, radius: 4pt)[
  核心挑战：将多源异构事件融合为可解释、可审计、可执行的结构化预测结果。
]

#v(0.3em)

#table(
  columns: (0.36fr, 1fr),
  stroke: none,
  inset: (x: 0.3em, y: 0.35em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*归一化*], [所有来源转为统一 `AppEvent` schema，并按时间窗口聚合],
  [*脱敏*], [位置、通知正文、应用内文本均在本地转换为粗粒度标签],
  [*推理*], [云端 LLM 只接收结构化上下文，输出预测应用、置信度、风险与摘要],
  [*回放*], [Golden Trace 支持离线复现，便于比较策略效果],
)

== 资源调度策略

#block(fill: luma(242), inset: 0.75em, radius: 4pt)[
  Policy Engine 将预测结果映射为三类低风险系统动作，并受预算、风险等级和用户确认约束。
]

#v(0.3em)

#table(
  columns: (auto, auto, 1fr),
  stroke: none,
  inset: (x: 0.24em, y: 0.26em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*动作*], [*触发*], [*内容*],
  [Keep-Alive], [高置信度], [短窗口保护进程，减少重建成本],
  [Prewarm], [中置信度], [按热点文件画像预读关键缓存],
  [Hint], [低置信度], [只记录候选，不做实际资源占用],
)

#v(0.3em)

#block(fill: blue.lighten(85%), inset: 0.65em, radius: 4pt)[
  参考 AppFlow：选择性文件预加载、自适应内存回收、上下文感知进程杀死。
]

== 渐进式扩展路径

#table(
  columns: (auto, 0.45fr, 0.45fr, auto),
  stroke: none,
  inset: (x: 0.2em, y: 0.24em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*阶段*], [*已具备*], [*下一步*], [*环境*],
  [App], [UsageStats、内存快照], [验证预测闭环], [APK],
  [Daemon], [`NativeSystemBridge`], [接入系统状态], [root],
  [Framework], [Predictor / Policy 接口], [自定义系统服务], [ROM],
  [Kernel], [AppFlow 策略逻辑], [预读、LMK、eBPF], [AOSP],
)

#v(0.3em)

#block(fill: yellow.lighten(70%), inset: 0.65em, radius: 4pt)[
  每一层接口保持稳定：替换底层实现时，上层预测和策略代码无需重写。
]

// ══════════════════════════════════════════════════════════════
// PART 4: STATUS
// ══════════════════════════════════════════════════════════════
= 进展与展望

== 当前进展

#table(
  columns: (0.4fr, 0.95fr),
  stroke: none,
  inset: (x: 0.35em, y: 0.35em),
  fill: (_, y) => if calc.odd(y) { luma(242) } else { white },
  [*架构设计*], [完成分层边界、事件 schema、Policy Engine 与 SystemBridge 的核心接口设计],
  [*Android 原型*], [打通 UsageStats、通知、基础系统状态采集与本地脱敏流程],
  [*策略验证*], [形成 Keep-Alive / Prewarm / Hint 三类动作和预算约束],
  [*工程支撑*], [使用 Golden Trace 回放与 CI 依赖审计保证可复现、可维护],
)

== 下一阶段计划

#block(fill: luma(242), inset: 0.8em, radius: 4pt)[
  1. 完成 Android 原型端到端演示：采集 $arrow.r$ 脱敏 $arrow.r$ 预测 $arrow.r$ 本地动作。

  2. 建立 Golden Trace 数据集，覆盖通勤、学习、社交、办公等典型使用场景。

  3. 对比无预热、启发式预热、LLM 意图预测三种策略的启动时延与资源开销。

  4. 验证隐私、误预测和资源预算约束，再进入 root / ROM 环境测试 eBPF 与文件预读。
]

== 核心技术贡献

#block(fill: blue.lighten(85%), inset: 0.8em, radius: 4pt)[
  *隐私 Air-Gap 架构*：敏感信息本地确定性脱敏，云端只见结构化上下文。

  #v(0.25em)
  *确定性状态机 + Golden Trace 回放*：让 LLM 参与的系统策略仍可复现、可审计。

  #v(0.25em)
  *三层信息融合*：API 层、内核层、语义层共同补足从 PID 到 Intent 的语义链路。

  #v(0.25em)
  *机制-策略分离*：云端提出建议，本地保留执行权，兼顾智能性与系统安全边界。
]
