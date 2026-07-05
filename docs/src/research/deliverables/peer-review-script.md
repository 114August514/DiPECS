# 讲稿

## 开场：我们到底做了什么

各位老师、同学大家好。我们小组的项目叫 DiPECS，全称是 Digital Intelligence Platform for Efficient Computing Systems。这个名字比较长，但我们真正想回答的问题可以概括成一句话：

**当 AI 能力进入操作系统之后，操作系统应该如何感知上下文、如何把预测转化为资源和体验收益、又如何保证动作执行仍然安全可控？**

DiPECS 不是从零写一个内核，也不是做一个普通聊天助手。它是面向 Android 平台的 AIOS 原型层：下面连接真实 Android 系统信号，上面连接规则或大模型，中间用操作系统式的边界把“采集、窗口聚合、决策、授权、执行、度量”串成一条闭环。

如果用 OS 课程里的概念来类比，DiPECS 的目标不是让 LLM 变成内核，而是让 LLM 像一个受限的用户态策略模块一样工作。它可以提出意图，但不能直接操作系统资源；它可以参考上下文，但不能读取原始敏感数据；它可以建议动作，但动作必须经过本地策略审查，像系统调用进入内核态一样，通过权限、能力和风险检查后才能执行。

所以我们的项目核心是一个问题：**如何把 AI 的不确定性放进操作系统确定性的治理框架里，并让它真正改善 Android 资源和用户体验指标。**

接下来我会从五个重点展开：

1. 为什么这个问题是操作系统问题。
2. DiPECS 的中心架构是什么。
3. 我们如何对应 OS 课程里的机制与策略、保护边界、系统调用、审计和调度思想。
4. 我们的技术贡献和实现亮点。
5. 当前局限和未来工作。

## 一、为什么这是一个操作系统问题

现在很多应用都能接入大模型，比如总结文档、搜索资料、生成代码、执行简单任务。但这些能力大多停留在单个应用内部。一个 App 只能看到自己的局部状态，很难理解整个系统正在发生什么。

操作系统不一样。OS 管理进程、文件、内存、I/O、权限、通知、窗口和设备状态。它天然拥有更完整的上下文：哪个应用在前台、系统是否低电量、通知是否频繁打断用户、某些资源是否被重复访问、后台进程是否占用过高。这些信息正是 AI 判断用户意图时需要的上下文。

但问题也在这里。OS 能看到的信息越多，风险越大。

从 OS 课程角度看，操作系统最重要的职责之一就是资源保护。内核不能让任意用户态程序随便读写内存、访问文件、控制设备。同理，在 AIOS 中，我们也不能让大模型随便读取原始系统事件，更不能让模型输出直接变成系统动作。

这里有四个核心挑战：

第一是资源收益。只有预测准确还不够，系统必须证明动作真的改善了启动延迟、文件等待、内存压力或 jank，并且收益超过动作成本和控制面开销。

第二是保护边界。通知内容、应用切换、文件路径、设备状态都可能包含敏感信息。如果直接拼进 prompt 发给模型，相当于绕过了操作系统的数据保护边界。

第三是安全。大模型输出具有不确定性，可能会 hallucinate 一个不存在的目标，也可能建议一个风险过高的动作。系统不能因为模型说“预热某个进程”就真的去操作。

第四是可验证性。操作系统强调可重复、可调试、可审计。一个系统级 AI 原型不能只说“模型觉得应该这样做”，还要说明输入是什么、脱敏后是什么、为什么生成这个意图、为什么允许或拒绝动作、最终执行结果是什么。

DiPECS 的设计就是围绕这三个挑战展开的。

## 二、中心架构：一条受控的 AIOS 管线

DiPECS 的中心架构可以用一条管线表示：

```text
Android collector JSONL / daemon sources / replay fixture
    -> aios-collector
    -> RawEvent
    -> PrivacyAirGap
    -> SanitizedEvent
    -> WindowAggregator
    -> StructuredContext
    -> DecisionRouter
    -> IntentBatch
    -> PolicyEngine + CapabilityLevel
    -> ActionLifecycle
    -> AuthorizedAction
    -> ActionAdapter
    -> AuditRecord / runtime trace / replay audit hash
```

这条链路看起来长，但它的思想很简单：**原始数据不能直接进模型，模型输出不能直接执行动作，每一步都要有类型边界和审计记录。**

我们把项目分成几个核心 crate：

- `aios-spec`：定义协议和数据结构，是整个系统的 single source of truth。
- `aios-collector`：负责采集入口，把 Android 或 daemon 输入规范化为事件。
- `aios-core`：负责隐私脱敏、窗口聚合、策略审查和动作生命周期。
- `aios-agent`：负责决策路由，把结构化上下文变成意图。
- `aios-action`：负责执行经过授权的动作。
- `aios-daemon`：把整条在线管线跑起来。
- `aios-cli`：提供 replay、audit 和 Android bridge 工具。

这个分层对应 OS 课程中一个非常重要的思想：**机制和策略分离**。

机制层负责提供稳定、安全、可验证的基础设施，例如采集、脱敏、策略审查、动作封装和审计。策略层负责判断当前上下文下“可能应该做什么”。在 DiPECS 中，规则后端、本地评估器和云端 LLM 都属于策略层；Privacy Air-Gap、PolicyEngine、ActionLifecycle 和 ActionAdapter 属于机制层。

这样的好处是：以后我们可以替换策略，比如从规则换成本地小模型，或者接入更强的 LLM，但动作授权和隐私边界不需要跟着推倒重来。

## 三、第一层贡献：把 OS 保护边界引入 AIOS

DiPECS 最重要的技术贡献之一，是把操作系统中的保护边界思想迁移到 AIOS 管线里。

传统 OS 中，用户态程序不能直接访问内核数据结构，不能随便操作设备，也不能直接修改别的进程内存。它必须通过系统调用进入内核，由内核检查权限和参数。

DiPECS 中也有类似边界。

第一个边界是 `PrivacyAirGap`。

Android collector 或 daemon source 产生的是 `RawEvent`。这些原始事件可能包含敏感信息，例如通知文本、文件路径、包名、设备状态、交互记录等。`RawEvent` 只允许存在于 collector 到 core 的短路径上。一旦进入 `PrivacyAirGap`，它就必须被转换为 `SanitizedEvent`。

模型后端看不到 `RawEvent`。它只能看到 `StructuredContext`，也就是脱敏和窗口聚合之后的结构化上下文。

这和 OS 里的地址空间隔离很像。用户进程不能直接读内核内存，模型也不能直接读原始系统事件。我们不是靠 prompt 告诉模型“请不要泄露隐私”，而是在数据流上让它根本拿不到原始数据。

第二个边界是 `AuthorizedAction`。

模型或规则后端只能输出 `IntentBatch`，里面是建议动作。建议动作不是可执行动作。它必须进入 `PolicyEngine`，经过风险、置信度、能力等级、target-in-context 和 allow-list 检查。只有通过审查，`ActionLifecycle` 才能构造 `AuthorizedAction`。

这和系统调用检查也很像。应用可以请求打开文件，但内核要检查权限；模型可以建议预取资源，但 DiPECS 要检查这个动作是否低风险、目标是否在当前上下文中、后端能力是否允许。

第三个边界是 `ActionAdapter`。

执行层只接受 `AuthorizedAction`。也就是说，即使 Cloud LLM 输出了一个看起来很合理的动作，它也不能直接进入 Android bridge。必须经过本地治理层 seal。

这三个边界共同保证了一个原则：**AI 可以参与策略判断，但不能绕过操作系统式的保护机制。**

## 四、第二层贡献：把中断、缓冲和批处理思想用于上下文窗口

DiPECS 的采集不是直接把每条事件都发给模型，而是先进入事件流，再通过窗口聚合形成 `StructuredContext`。

这里可以和 OS 课程中的中断处理、缓冲区和批处理思想联系起来。

操作系统面对外设输入时，不会让每个硬件事件都触发一整套昂贵逻辑。通常会有中断处理、缓冲、队列、延迟处理和调度。原因是系统事件频繁、细碎，而且单个事件本身语义不足。

Android 上的应用切换、通知、设备状态也是类似的。单独一条通知不一定说明用户要做什么；单独一次屏幕亮起也不一定说明要预热什么。我们需要把一段时间内的事件放在一起看。

DiPECS 使用 `WindowAggregator` 做窗口聚合。它把若干 `SanitizedEvent` 聚合成一个 `StructuredContext`。这个上下文包括：

- 前台应用变化。
- 通知来源和语义提示。
- 文件活动类别。
- 最新系统状态。
- source tier，也就是事件来源能力等级。

这样做有三个好处。

第一，降低模型输入噪声。模型不需要看到每条原始事件，只需要看结构化摘要。

第二，提高决策稳定性。窗口上下文比单点事件更接近用户真实状态。

第三，降低系统开销。批量处理可以减少频繁调用后端的成本，也更适合 replay。

从 OS 角度说，这类似把原始中断事件先放进缓冲区，再由内核或后台 worker 统一处理，而不是每个事件都立即触发重操作。

## 五、第三层贡献：DecisionRouter 体现调度与降级思想

DiPECS 不默认把所有上下文都发给云端 LLM，而是用 `DecisionRouter` 在多个后端之间选择：

- `RuleBasedBackend`
- `LocalEvaluatorBackend`
- `CloudLlmBackend`
- `FallbackNoOpBackend`

这个设计可以和 OS 中的调度器类比。

调度器要根据任务优先级、资源状态和策略选择下一个运行对象。`DecisionRouter` 则根据上下文复杂度、配置、网络状态和隐私约束选择合适的决策后端。

简单、确定、低风险的场景交给 `RuleBasedBackend`。例如低电量时减少激进动作、前台应用保持轻量维护、通知密度较高时生成保守建议。这些不需要云端模型。

需要本地判断但不一定需要云端的场景交给 `LocalEvaluatorBackend`。它可以做候选排序和低风险预判。

只有当上下文复杂、需要跨事件语义理解，并且用户显式配置了 provider 时，才使用 `CloudLlmBackend`。当前支持 DeepSeek、Qwen/DashScope 和 OpenAI-compatible endpoint。

如果云端配置错误、网络失败、连续错误触发熔断，系统进入 `FallbackNoOpBackend` 或回落本地规则。

这个设计的优势是本地优先和 fail-safe。

操作系统中，失败时保持系统稳定比追求一次激进优化更重要。DiPECS 也一样。我们宁愿在不确定时 no-op，也不让模型输出越过安全边界。

## 六、第四层贡献：PolicyEngine 是动作能力的本地内核

如果说 `DecisionRouter` 像调度器，那么 `PolicyEngine` 更像一个本地安全内核。

它不负责生成想法，而负责审查想法能不能做。

当前策略检查包括：

- 后端能力等级。
- 全局自动执行风险上限。
- 置信度下限。
- blocked action 子串。
- urgency 检查。
- 单 intent action 数量上限。
- action 是否在后端 allow-list 中。
- target 是否出现在当前 `StructuredContext` 中。

这里最重要的是 `CapabilityLevel`。不同后端有不同能力上限。规则后端只能产生低风险动作；云端 LLM 即使语义能力更强，也仍然要经过本地策略复审。这样可以防止某个后端因为模型输出或实现错误扩大权限。

这和操作系统中的 capability-based security 很接近。一个进程能做什么，不只取决于它“想做什么”，还取决于它持有什么 capability。DiPECS 中每个 decision route 也有自己的动作能力范围。

动作被审查后进入 `ActionLifecycle`。每个动作都有确定性坐标：

```text
(window_ordinal, intent_ordinal, action_ordinal)
```

它最终必须产生一条终态 `AuditRecord`。这个终态可能是成功、失败、schema 拒绝、policy 拒绝或 capability 拒绝。

这让 DiPECS 的动作执行不再是“模型说了什么”，而是“系统记录了一个完整生命周期”。这也是系统软件和普通 Agent 的区别。

## 七、第五层贡献：Android Action Bridge 的安全收缩

DiPECS 不是只做离线分析，它也实现了 Android Action Bridge，把 Rust 侧经过授权的动作转发给 Android collector。

但这里我们做了很强的语义收缩。

当前可转发动作包括：

- `PrefetchFile`
- `KeepAlive`
- `ReleaseMemory`
- `PreWarmProcess`
- `NoOp`

这些名字听起来可能比较“系统级”，但 Android 侧实际语义是安全子集：

- `PrefetchFile` 只预取 HTTPS URL 或 app 有权限访问的 `content://` URI，并写入 app cache。
- `KeepAlive` 只调度 DiPECS 自己的 `JobScheduler` maintenance job。
- `ReleaseMemory` 只清理 DiPECS 自己的 cache。
- `PreWarmProcess(own:*)` 只预热自身资源。
- `PreWarmProcess(pkg:*)` 或 `notif:*` 不后台启动第三方 App，只发用户可见提示。

也就是说，我们没有做：

- 静默启动第三方应用。
- 修改第三方进程保活。
- 清理第三方应用内存。
- 读取第三方私有文件。
- 绕过用户授权访问内容。

安全上，Android socket 只监听 `127.0.0.1`，使用 token、freshness window 和 HMAC-SHA256。payload 有大小限制、读超时、失败退避和 client 数量限制。

这可以联系 OS 中的 I/O 权限和设备驱动边界。即使上层请求通过了，真正执行到设备或平台 API 时，还要进行最后一层适配和限制。Action Bridge 就是 DiPECS 的受控 I/O 出口。

## 八、第六层贡献：Replay 和 Audit 让系统可验证

操作系统课程里经常强调调试和可观测性。系统软件一旦出问题，不能只看最终现象，还要能追踪事件、状态和执行路径。

DiPECS 的 `aios-cli replay` 就是为这个目的设计的。

它可以读取 Android JSONL trace，复用生产管线，但用 deterministic execution 替代真实动作执行。Replay 支持多个 stage：

- `ingest`
- `sanitize`
- `context`
- `decision`
- `policy`
- `execute`

这意味着我们可以逐层检查：

- 事件是否正确进入系统。
- 隐私脱敏是否生效。
- 上下文窗口是否稳定。
- 决策路由是否合理。
- 策略拒绝是否符合预期。
- 动作生命周期是否完整。

同时，Replay 会生成 canonical audit stream 和 `audit_hash`。它会剥离 UUID、latency、window_id 这类不稳定字段，只保留确定性审计内容。

这个设计的贡献在于，DiPECS 不只是一个 demo，而是一个可以回归测试的系统原型。相同输入 trace 在相同策略下应该产生稳定 hash。这和文件系统 journaling、系统日志、trace replay 的思想是一脉相承的：系统行为必须能被记录、重放和验证。

## 九、技术贡献总结

如果把 DiPECS 的贡献压缩成几条，我认为最重要的是这六点。

第一，提出并实现了面向 AIOS 的 Privacy Air-Gap。它不是普通字段脱敏，而是架构级边界：RawEvent 不越过 core，模型只见 StructuredContext。

第二，实现了机制与策略分离。规则、本地评估器和 LLM 都只是策略后端；真正的动作授权由本地 PolicyEngine 和 ActionLifecycle 完成。

第三，构建了 Android public API 到 Rust daemon 的生产入口。UsageStats、NotificationListener 和 DeviceContext 通过 append-only JSONL 进入 Rust 管线，兼顾可部署性和 replay 能力。

第四，设计了 capability-aware 的动作治理。不同后端有不同动作能力，动作必须经过风险、置信度、target 和 allow-list 检查。

第五，实现了 Android-safe Action Bridge。动作语义被收缩到自有资源、用户授权内容和用户可见提示，并通过 token、HMAC 和 TTL 做保护。

第六，建立了 replay/audit 验证体系。系统可以用 trace 复现行为，用 audit hash 做回归，用 AuditRecord 追踪每个动作终态。

这些贡献都围绕一个中心：让 AI 能力进入 OS 时，仍然保留操作系统最重要的属性：保护、隔离、授权、调度、审计和可恢复。

## 十、和 OS 课程知识的对应关系

这里我把项目和 OS 课程知识做一个更明确的对应。

第一，对应“用户态/内核态隔离”。模型后端相当于不可信策略模块，不能直接接触原始事件，也不能直接执行动作；core 中的 Privacy Air-Gap 和 PolicyEngine 承担保护边界。

第二，对应“系统调用”。IntentBatch 类似用户态请求，AuthorizedAction 类似通过检查后的内核授权对象。ActionLifecycle 就是从请求到授权再到审计的状态机。

第三，对应“进程调度”。DecisionRouter 根据上下文、后端能力和失败状态选择 RuleBased、LocalEvaluator、CloudLlm 或 FallbackNoOp，体现了调度和降级策略。

第四，对应“缓冲和批处理”。WindowAggregator 把短时间内的事件聚合成 StructuredContext，减少频繁决策和噪声。

第五，对应“能力安全”。CapabilityLevel 限制不同后端能产生的动作类型和风险等级，防止策略后端越权。

第六，对应“日志与恢复”。Replay、AuditRecord 和 audit hash 让系统行为可追踪、可回放、可回归。

所以 DiPECS 虽然不是内核项目，但它的核心思维是操作系统式的：把不可信、复杂、动态的外部输入纳入一套稳定的本地治理机制。

## 十一、局限性

当然，DiPECS 目前仍然是原型系统。我们认为主要局限有三个。

第一，真实设备数据还不足。当前已经有 Android collector、模拟器脚本、synthetic trace 和 replay fixture，但长期真机采集、不同厂商 ROM、不同权限组合下的行为还没有充分验证。

第二，实验规模还比较小。现有测试能证明管线、策略和审计机制成立，但还不足以证明长期用户体验收益，例如动作命中率、误触发率、资源开销和用户感知延迟改善。

第三，系统级高权限采集仍是预留路线。Binder/eBPF、fanotify 和 system image 能提供更底层信号，但当前主线仍以 Android public API 和 daemon source 为主。这让系统更容易部署，但也限制了可观测范围。

这些局限更多是原型阶段的规模问题，而不是中心架构失效。后续如果有更多真机数据、更长时间运行和更丰富场景，可以进一步评估 DiPECS 的效果上限。

## 十二、未来工作

未来我们希望沿着三个方向继续扩展。

第一，补充真机和长期数据。包括不同 Android 版本、不同权限组合、不同使用习惯下的 trace，进一步评估动作命中率、资源开销和隐私边界稳定性。

第二，增强本地智能。可以引入更轻量的本地模型或行为预测器，让系统在不依赖云端的情况下更好地理解上下文，同时仍然保持 Privacy Air-Gap 和 AuthorizedAction 边界。

第三，扩展动作和采集能力。动作侧可以增加更多 Android-safe 的自有资源操作；采集侧可以在受控研究环境中探索 Binder/eBPF、fanotify 等系统级信号，但必须先定义 schema、脱敏规则和能力边界。

## 结尾

最后总结一下 DiPECS。

我们的项目不是把 LLM 简单接到操作系统上，而是尝试回答“AIOS 应该如何被操作系统治理”。

我们设计了一条本地优先的系统管线：从 Android public API 和 daemon source 采集信号，经 Privacy Air-Gap 脱敏，经过 WindowAggregator 形成结构化上下文，再由 DecisionRouter 生成意图，最后通过 PolicyEngine、CapabilityLevel 和 ActionLifecycle 形成 AuthorizedAction，并由 ActionAdapter 执行和审计。

从技术贡献上看，DiPECS 把 OS 课程中的保护边界、机制策略分离、系统调用检查、调度与降级、能力安全、日志与回放这些思想，应用到了 AIOS 原型设计中。

一句话总结就是：**DiPECS 让 AI 可以参与操作系统决策，但不能绕过操作系统治理。**

谢谢大家。

## 可选问答准备

### Q1：为什么默认不使用云端 LLM？

因为系统上下文包含敏感信息，而且很多低风险动作不需要大模型。默认本地规则优先，可以减少网络依赖和隐私压力。CloudLlmBackend 作为可选增强，只在配置完整且需要复杂语义理解时使用。

### Q2：Privacy Air-Gap 和普通脱敏有什么区别？

普通脱敏往往是一个数据处理步骤，而 Privacy Air-Gap 是架构边界。RawEvent 不能越过这个边界，模型只能看到 StructuredContext。它不是靠 prompt 约束模型，而是从数据流上切断原始敏感信息。

### Q3：为什么模型不能直接执行动作？

模型输出不稳定，系统动作有副作用。DiPECS 把模型输出限制为 IntentBatch，动作必须经过 PolicyEngine 和 ActionLifecycle 才能成为 AuthorizedAction。这相当于系统调用必须经过内核权限检查。

### Q4：项目最大优势是什么？

最大优势是把 AIOS 问题系统化了。我们没有只做一个模型 demo，而是做了采集、脱敏、决策、授权、执行和审计的完整链路，并且每个环节都有明确边界。

### Q5：当前最大不足是什么？

主要是真机数据和长期实验规模不足。当前机制已经能通过 replay、audit 和模拟器验证，但仍需要更多真实设备、更多用户场景和更长运行时间来证明实际体验收益。
