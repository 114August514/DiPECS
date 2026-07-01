# Peer Review

## 40 分钟讲稿版

各位老师、同学大家好，我们小组的项目叫 DiPECS，全称是 Digital Intelligence Platform for Efficient Computing Systems。这个名字直译过来，是面向高效计算系统的数字智能平台。更具体地说，我们希望在 Android 和 Linux 场景下，做一个本地优先的 AIOS 原型系统。

这里的 AIOS，可以理解为 AI Native Operating System，也就是把人工智能能力纳入操作系统运行链路的一类系统。传统操作系统主要负责资源管理、任务调度、权限控制和硬件抽象，而 AIOS 希望进一步做到感知用户状态、理解系统上下文、预测需求，并在安全边界内执行一些辅助动作。我们的项目不是要从零实现一个完整操作系统内核，而是从系统软件的视角，探索“智能操作系统应该怎样采集数据、怎样保护隐私、怎样做决策、怎样执行动作，以及怎样留下可审计记录”。

如果用一句话概括 DiPECS，它是一个面向 Android/Linux 的本地优先 AIOS 原型系统，重点解决智能系统中的感知、隐私、决策、策略审查、动作执行和审计回放问题。系统会采集应用切换、通知、设备状态等本地信号，通过 Privacy Air-Gap 做隐私脱敏，再聚合成结构化上下文，然后由规则引擎或可选的云端大模型生成意图。最后，这些意图不能直接执行，而是必须经过本地 PolicyEngine 审查，形成 AuthorizedAction 之后，才能进入动作执行层。

接下来我会按几个部分展开介绍。第一部分是项目背景和问题来源；第二部分是我们的总体目标；第三部分是系统架构和数据流；第四部分介绍各个模块；第五部分介绍安全、隐私和动作治理；第六部分介绍验证与评估；最后会总结创新点、局限和未来工作。

## 一、项目背景

近几年，大语言模型和智能 Agent 的发展非常快。我们现在已经能看到很多应用把大模型接入到工作流里，比如自动总结文档、自动处理邮件、自动执行搜索、自动生成代码等。这些应用大多运行在用户态，依赖应用自己能拿到的数据。它们能解决一部分任务，但也有一个明显问题：如果 AI 只停留在单个应用内部，它对系统整体状态的理解是有限的。

举个例子，假设用户刚刚打开了一个会议软件，又连续收到几条通知，同时电量比较低，后台还有几个高占用进程。一个普通 App 可能只能看到自己内部的数据，而操作系统层面可以看到更完整的上下文，包括当前前台应用是什么、系统资源是否紧张、通知是否频繁打扰用户、用户最近是否有重复的文件访问行为等。如果 AI 能在操作系统层面理解这些信息，就有机会做更自然、更及时的辅助。

但是，把 AI 放到操作系统附近，并不是简单地把所有数据都丢给大模型。这里会带来三个非常关键的问题。

第一个问题是隐私。操作系统能看到的数据非常敏感，包括应用使用情况、通知内容、文件路径、网络行为、进程状态等。如果直接把这些原始数据交给模型，尤其是云端模型，就会带来明显的隐私风险。AIOS 的核心挑战之一，就是怎样让模型获得足够的上下文，同时又不泄露原始敏感信息。

第二个问题是安全。大模型可能会产生不稳定、不确定甚至不合规的输出。如果模型说“释放这个进程的内存”“预加载这个文件”“打开某个应用”，系统不能盲目执行。操作系统里的动作必须有权限边界、能力分级、风险判断和审计记录。也就是说，模型可以提出建议，但不能直接拿到系统动作的控制权。

第三个问题是可维护性和可验证性。很多 Agent 系统会把感知、推理和执行写成一个比较松散的链路，短期可以跑通 demo，但长期很难验证它到底做了什么、为什么做、是否符合策略。对于系统软件来说，这种不透明是很危险的。我们希望 DiPECS 的每一步都有清晰边界，每一次动作都有审计记录，每一次回放都能复现。

基于这些背景，我们提出 DiPECS。它不是追求“模型能做所有事”，而是强调“模型只能在安全框架内参与决策”。我们把本地机制和智能策略分开，把原始数据和模型输入分开，把意图和动作分开，把动作建议和授权执行分开。这是整个项目的核心思想。

## 二、项目目标

DiPECS 的目标可以拆成四个层次。

第一个目标，是实现一个可运行的端侧 AIOS 原型链路。也就是说，我们不只做概念设计，而是要有实际代码，从 Android 采集器到 Rust daemon，从事件解析到上下文聚合，从决策路由到动作授权，再到 replay 和 audit，都要形成完整闭环。

第二个目标，是建立清晰的隐私边界。我们设计了 Privacy Air-Gap，也就是隐私隔离层。RawEvent 这样的原始事件只允许在采集和脱敏阶段短暂存在。进入模型之前，数据必须被转换成 SanitizedEvent 和 StructuredContext。模型后端看到的是结构化摘要，而不是原始通知内容、原始路径、原始应用细节。

第三个目标，是实现机制和策略分离。机制层负责采集、脱敏、聚合、审计和受控执行；策略层负责基于脱敏上下文生成意图。无论策略层是规则引擎、本地模型，还是云端大模型，它都只能输出 IntentBatch，也就是意图集合。真正能执行的 AuthorizedAction 只能由本地动作治理层构造。

第四个目标，是让系统可验证、可回放、可审计。我们实现了 trace replay 和 audit hash。通过 `aios-cli replay`，可以把 Android JSONL trace 或测试 fixture 重新跑一遍，得到确定性的审计输出。这样可以检查隐私边界是否被突破、策略是否按预期拒绝高风险动作、同样输入是否产生稳定结果。

所以 DiPECS 的关键词不是“让 AI 控制系统”，而是“让 AI 在本地安全框架内辅助系统”。这一点非常重要。我们希望探索的是智能操作系统的一条可控路线，而不是把系统权限直接交给不确定的模型输出。

## 三、总体架构

从整体架构来看，DiPECS 可以分成几个主要模块：`aios-spec`、`aios-collector`、`aios-core`、`aios-agent`、`aios-action`、`aios-daemon`、`aios-cli`，以及 Android 侧的 `apps/android-collector`。

`aios-spec` 负责定义核心数据结构。比如 RawEvent、CollectorEnvelope、SanitizedEvent、StructuredContext、IntentBatch、CapabilityLevel、AuthorizedAction 等。我们把这些类型集中定义，是为了保证不同模块之间的接口清晰稳定。

`aios-collector` 负责采集入口和标准化。它会把 Android 端写入的 append-only JSONL 解析为 Rust 可以处理的事件，也保留了 daemon 系统源、Binder/eBPF 和 fanotify 等未来扩展接口。当前主线实现中，Android public API 是主要采集来源。

`aios-core` 是系统的核心处理层，负责 Privacy Air-Gap、事件脱敏、窗口聚合、模型记忆、策略检查相关基础能力，以及 golden trace replay 和 privacy leak regression tests。

`aios-agent` 负责决策路由。它提供 DecisionRouter、RuleBasedBackend、LocalEvaluatorBackend、CloudLlmBackend 和 FallbackNoOpBackend。默认情况下系统优先使用本地规则，云端大模型只有在环境变量显式启用并且配置完整时才参与。

`aios-action` 负责动作治理和执行适配。它不会接受模型直接给出的任意动作，而是只处理经过 PolicyEngine 和 ActionLifecycle 形成的 AuthorizedAction。Android 侧的安全动作子集可以通过 localhost action bridge 转发。

`aios-daemon` 是长运行进程，负责把采集、核心处理、决策和动作链路串起来。`aios-cli` 则提供 replay、audit、Android action socket 工具等命令行能力。

Android 侧的 `apps/android-collector` 是 public API collector。它使用 Android 公开 API 采集应用切换、通知、设备状态等信息，写入 append-only JSONL，并提供 action socket，用于接收 Rust 侧经过授权的动作请求。

如果用数据流来描述，DiPECS 当前主链路是这样的：

Android collector JSONL、daemon system sources 或 replay fixture，首先进入 `aios-collector`，转换成 IngestedRawEvent。然后经过 Privacy Air-Gap，变成 SanitizedEvent。接着 WindowAggregator 把一段时间内的事件聚合成 StructuredContext。StructuredContext 交给 DecisionRouter，输出 IntentBatch。IntentBatch 再进入 PolicyEngine 和 CapabilityLevel 检查。通过审查后，ActionLifecycle 生成 AuthorizedAction。最后 ActionAdapter 执行动作，并输出 AuditRecord、runtime trace 或 replay audit hash。

这条链路里有几个关键边界。

第一，RawEvent 不允许跨过 Privacy Air-Gap。它只存在于 collector 到 core 的短路径上。

第二，模型后端只能看到 StructuredContext，不能看到原始事件。

第三，决策后端只能输出 IntentBatch，不能构造 AuthorizedAction。

第四，AuthorizedAction 的唯一构造点是 ActionLifecycle。

第五，每个动作坐标 ActionCoord 都必须产生一条终态 AuditRecord。

这些边界保证了系统不是一条随意的 Agent pipeline，而是一个带有操作系统风格约束的智能运行链路。

## 四、Android 采集层

下面具体介绍 Android 采集层。

DiPECS 当前使用 Android public API 作为生产入口，主要原因是它更容易在普通设备和模拟器上部署，也更符合权限边界。我们没有一开始就依赖系统镜像、root 权限或内核模块，而是先用公开 API 做出可运行链路。

当前已经提升为 production ingress 的 Android 数据源主要有三个。

第一个是 UsageStatsManager，对应 `RawEvent::AppTransition`。它可以帮助系统了解应用切换和前后台状态变化。比如用户从浏览器切到文档，从聊天软件切到会议软件，这些事件可以反映用户当前任务环境。

第二个是 NotificationListenerService，对应 `RawEvent::NotificationPosted` 和 `RawEvent::NotificationInteraction`。它可以记录通知出现和用户交互情况。通知是移动系统里非常重要的上下文来源，因为它反映了外部事件对用户注意力的影响。

第三个是 DeviceContext，对应 `RawEvent::SystemState`。它记录设备状态，比如电量、充电状态、网络状态、屏幕状态等。这些信息对动作决策很重要。例如在低电量时，系统不应该积极做高成本预加载；在充电且网络稳定时，可以允许一些更积极的后台准备动作。

此外，AccessibilityService 当前作为 screening source 保留。也就是说，Android app 侧可以记录和预览 Accessibility 事件，但这些行在 Rust 生产入口里以 `rawEvent: null` 表示，并不会进入正式 schema。这样做的原因是 Accessibility 事件通常更敏感，包含更细粒度的用户交互信息。我们希望在没有明确 schema 和脱敏策略之前，不把它纳入生产链路。

Android collector 和 Rust daemon 之间使用 append-only JSONL 作为生产入口。这个设计很朴素，但有几个好处。第一，它容易调试，每一行都是独立事件。第二，它适合 replay，可以把真实或合成 trace 保存下来反复验证。第三，它降低了 Android 和 Rust 之间的耦合，不需要一开始就设计复杂 RPC。

在运行时，`dipecsd --android-trace-jsonl` 可以 tail 新追加的 rawEvent 行，把它们送入 Rust 管线。这使得 Android 侧采集和 Rust 侧智能处理之间形成了一个清晰的边界。

## 五、Privacy Air-Gap 与上下文聚合

DiPECS 的一个核心设计是 Privacy Air-Gap。它的作用可以理解为一道隐私防火墙，目标是防止原始敏感数据直接流向模型和动作执行器。

在传统 Agent 系统里，经常会出现这样的写法：采集到一批原始日志，然后直接拼成 prompt 发给模型。这样实现简单，但隐私风险很高。因为原始日志里可能包含应用名、通知文本、文件名、路径、用户输入片段等敏感信息。尤其当模型是云端模型时，这些数据会离开本地设备。

DiPECS 的做法是：RawEvent 只在采集和脱敏阶段存在。一旦进入 Privacy Air-Gap，就要被转换成 SanitizedEvent。脱敏后的事件再由 WindowAggregator 聚合成 StructuredContext。这个 StructuredContext 才是模型后端可以看到的输入。

StructuredContext 并不是原始日志的简单拼接，而是结构化摘要。它可以包含当前窗口内的应用切换模式、通知密度、系统资源状态、动作历史反馈、行为 profile 等。这样模型可以理解“用户现在可能处在一个高注意力负载的工作场景”，但不需要知道每一条通知的具体文本。

这种设计有两个重要意义。

第一，它让隐私保护成为架构边界，而不是 prompt 里的提醒。我们不是告诉模型“请不要泄露隐私”，而是在模型之前就把原始隐私数据截断。

第二，它降低了不同模型后端的风险差异。无论使用规则引擎、本地评估器，还是云端 LLM，它们接收到的都是同一种经过治理的 ModelInput，而不是各自随意读取原始数据。

当然，Privacy Air-Gap 不是说系统完全没有隐私风险。任何上下文摘要都有可能携带一定信息。但相比直接把原始事件发给模型，结构化脱敏摘要的风险明显更可控，也更容易测试。我们可以通过 privacy leak regression tests 检查某些敏感字段是否越过边界。

## 六、决策路由层

DiPECS 的决策层由 `aios-agent` 实现，核心组件是 DecisionRouter。

DecisionRouter 的职责不是直接执行动作，而是在不同后端之间选择合适的决策来源，并把 StructuredContext 转换成 IntentBatch。这里的 IntentBatch 可以理解为“系统认为接下来可能有用的动作建议集合”。

当前实现中，默认优先使用 RuleBasedBackend，也就是本地规则后端。这个选择是有意为之的。很多系统级动作不一定需要大模型，比如低电量时降低预加载积极性、通知密度高时避免打扰、检测到某些重复上下文时产生维护建议，这些都可以通过规则稳定实现。规则后端的好处是可解释、可测试、可预测。

CloudLlmBackend 是可选能力，默认关闭。只有当环境变量启用并且 provider API key 等配置完整时，云端模型才参与。当前支持 DeepSeek、Qwen/DashScope，以及 OpenAI-compatible endpoint。这样设计是为了让系统在没有云端模型时也能运行，同时在需要更复杂语义理解时可以扩展到 LLM。

此外还有 LocalEvaluatorBackend 和 FallbackNoOpBackend。FallbackNoOpBackend 很重要，因为系统必须考虑失败情况。如果云端模型失败、配置错误、连续错误触发熔断，系统不能崩溃，也不能乱执行动作，而是应该回落到本地规则或 no-op。对于操作系统软件来说，“失败时保持安全”比“成功时很智能”更基础。

这里还要强调一点：无论后端是什么，它都只能输出 IntentBatch。大模型不能绕过策略层，不能直接构造 AuthorizedAction，也不能直接调用 Android bridge。这是机制和策略分离的关键。

我们可以把 DecisionRouter 理解成一个受限的建议生成器。它负责回答“根据当前上下文，有哪些动作值得考虑”，但不负责回答“这些动作是否可以执行”。后一个问题必须交给 PolicyEngine。

## 七、策略审查与动作治理

在 DiPECS 里，动作执行不是从模型输出直接开始的，而是从策略审查开始的。这个部分主要由 PolicyEngine、CapabilityLevel、ActionLifecycle 和 AuthorizedAction 组成。

为什么要有这一层？因为系统动作有风险差异。比如记录一条本地 trace 的风险很低，预取一个文件可能涉及路径和资源消耗，释放内存可能影响正在运行的任务，预热进程可能造成电量和性能开销。不同动作需要不同能力等级和上下文条件。

PolicyEngine 会根据多个因素做判断，包括风险等级、置信度、能力等级、urgency、target-in-context 检查以及 action allow-list。简单来说，它会看这个动作是不是被允许、目标是不是在当前上下文里合理、模型置信度是否足够、动作风险是否超过当前能力级别。

只有通过策略审查的动作，才会进入 ActionLifecycle。ActionLifecycle 是 AuthorizedAction 的唯一构造点。这一点是为了防止其他模块绕过审查直接执行动作。

AuthorizedAction 可以理解为一张本地授权票据。它不是模型说出来的一句话，而是经过本地策略系统确认后的动作对象。执行层只接受 AuthorizedAction，而不接受 IntentBatch 或模型原始输出。

这种设计有点类似操作系统里的权限检查。用户态程序不能直接操作硬件，它必须通过系统调用进入内核，由内核检查权限。DiPECS 里的模型和规则后端也不能直接执行动作，它们必须提交意图，由本地策略层审查。

动作治理层还会产生 AuditRecord。每个动作坐标都应该有一条终态记录，说明动作是通过、拒绝、失败还是回退。这样后续可以追踪系统行为，也可以在出问题时定位原因。

## 八、Android Action Bridge

DiPECS 不只做离线 replay，也实现了 Android action bridge，用来把部分经过授权的动作发送到 Android 侧。

当前 Android bridge 通过 localhost socket 工作。Android collector 侧监听本地端口，Rust 侧的 `aios-action` 在启用环境变量后，可以把 Android-safe 的动作子集转发过去。

这里我们特别注意了安全边界。Android action socket 需要 `auth_token` 鉴权。token 存储在 Android 的 EncryptedSharedPreferences 中。CLI 或 bridge 工具需要拿到 token 才能发送请求。除此之外，动作 payload 还有大小限制、读取超时、失败退避和调度失败记录。

Rust 侧发送的不是任意字符串，而是 execute envelope。它包含短 freshness window，并且使用 HMAC-SHA256 对 freshness window 和长度前缀序列化的 AuthorizedAction 做校验。这样可以降低本地跨应用注入、重放和畸形 payload 的风险。

当前支持的 Android-safe 动作包括部分 prefetch、cache trim、maintenance job、自身资源 warmup、系统预装预热和用户可见提示等。我们没有把危险动作一股脑开放出来，而是以 allow-list 的方式逐步扩展。

Android bridge 的意义在于，它让 DiPECS 从“只会分析 trace 的系统”走向“能做受控动作的系统”。但这个动作能力仍然被严格限制在本地授权框架内。

## 九、Replay、Audit 与可验证性

对于系统软件来说，可验证性非常重要。DiPECS 的验证思路主要包括 replay、audit hash、golden trace、privacy leak regression tests 和 action-loop e2e。

`aios-cli replay` 可以回放 Android JSONL trace，并输出 ingest、sanitize、intent、policy、action、summary 等阶段的 NDJSON。这样我们可以看到一个事件从进入系统到最后产生动作审计的完整过程。

Replay 的一个重要价值是确定性。对于同样的输入 trace，如果规则、策略和配置不变，应该能得到稳定的审计输出。我们可以用 audit hash 来比较结果是否发生变化。这对回归测试很有用。

Golden trace 则是预先准备好的测试轨迹。它用于验证核心管线在已知场景下的行为是否符合预期。比如某些高风险动作应该被拒绝，某些 privacy leak 不应该出现在模型输入中，某些合法动作应该能形成 AuthorizedAction。

Action-loop e2e 测试通过 mock socket 验证动作回路。从上下文生成意图，到策略审查，再到动作转发和结果记录，整个链路可以在不依赖真实 Android 设备的情况下测试。

此外，项目也包含 emulator e2e 脚本，用于在 Android 模拟器上验证采集链路和 action socket。真实设备验证目前仍是未来工作的一部分，但模拟器验证已经能覆盖很多接口和流程问题。

在质量检查方面，Rust 部分通过 `cargo fmt`、`cargo clippy`、`cargo test` 等命令保证格式、静态检查和单元测试。Android APK 构建依赖本机 Android SDK，更多依赖 CI 验证。

## 十、项目实现特点

从工程实现上看，DiPECS 有几个比较明显的特点。

第一，模块边界比较清楚。spec、collector、core、agent、action、daemon、cli 分别承担不同职责。这样做的好处是，当我们讨论隐私边界、策略边界或动作边界时，可以对应到具体 crate，而不是停留在概念层面。

第二，默认本地优先。系统不依赖云端模型才能运行。RuleBasedBackend 是默认路径，CloudLlmBackend 是可选增强。这样既方便离线测试，也符合隐私友好原则。

第三，强调降级行为。Binder/eBPF、fanotify、系统镜像等高权限能力现在只是接口或预留路线，在没有权限部署时会安全降级。云端模型失败时也会回落到本地规则或 no-op。

第四，动作执行是 allow-list 风格。系统不会把模型输出映射成任意命令，而是只允许特定类型的动作通过策略审查后执行。这样牺牲了一些自由度，但换来了安全性和可维护性。

第五，文档和代码一起推进。项目里有当前实现总览、数据流、daemon 运行时、动作治理、Android 桥接、replay 与审计、RFC 和设计文档。这对课程项目很有帮助，因为评审者可以从文档快速理解系统，也可以回到代码验证实现。

## 十一、创新点

DiPECS 的创新点可以概括为五个方面。

第一是 Privacy Air-Gap。我们不是简单地在 prompt 里要求模型保护隐私，而是在系统架构上规定 RawEvent 不能越过隐私边界。模型只能看到脱敏后的 StructuredContext。这是从机制上降低隐私风险。

第二是机制和策略分离。模型或规则只负责生成意图，不能直接执行动作。动作必须经过 PolicyEngine 和 ActionLifecycle。这让 AIOS 更接近操作系统传统的安全模型。

第三是 Android public-API production ingress。我们没有一开始依赖 root 或系统镜像，而是先把 UsageStats、NotificationListener 和 DeviceContext 作为生产入口接入 Rust 管线。这让系统更容易部署和验证。

第四是 deterministic trace replay。Android JSONL 可以被稳定回放，并产生 audit hash。这样不仅可以展示系统效果，也可以做回归测试和隐私泄漏检测。

第五是 authenticated action bridge。Android localhost action socket 使用 token 鉴权、payload 限制、超时、失败退避和 HMAC 校验，降低本地动作注入风险。这体现了我们对“AI 执行动作”这个问题的谨慎态度。

这些创新点共同服务于一个目标：让 AIOS 不只是“更智能”，也要“更可控、更可审计、更安全”。

## 十二、和去年项目的关系与区别

从前面给出的去年大作业示例可以看出，很多项目都在探索操作系统、文件系统、分布式存储、Rust 重构、智能 Agent 或嵌入式系统。比如有的项目关注 Rust 重构文件系统或 RTOS，有的关注图文件系统和 LLM，有的关注基于 LLM 的内存管理优化。

DiPECS 和这些项目有一定联系，但重点不同。我们不是单纯重写某个内核模块，也不是只做一个文件系统 Agent。我们的核心是 AIOS 的运行链路治理，尤其是端侧智能系统里“数据怎样进来、隐私怎样隔离、模型怎样参与、动作怎样授权、行为怎样审计”。

如果和 MEMO 这类基于 LLM 预测用户行为的系统相比，DiPECS 更关注安全边界和可审计执行。MEMO 的重点是预测用户下一步行为并预加载资源，DiPECS 则更强调预测或决策结果不能直接执行，必须通过本地策略审查，并且每一步都要可回放。

如果和 IOSYS 或 MicroRust 这类文件系统 Agent 相比，DiPECS 不把文件作为唯一中心，而是面向更广泛的 Android/Linux 本地上下文，包括应用切换、通知、设备状态和动作回路。

如果和 Rust 重构 RTOS 的项目相比，DiPECS 的 Rust 主要用于构建安全、模块化、可测试的 AIOS 原型管线，而不是重写一个完整内核。我们的系统更像是运行在 Android/Linux 之上的智能治理层。

因此，DiPECS 的定位可以理解为：在操作系统与智能 Agent 之间，探索一条本地优先、隐私友好、动作受控的系统软件路线。

## 十三、一个具体运行场景

为了让系统更容易理解，我们可以举一个具体场景。

假设用户正在 Android 设备上工作。用户先打开浏览器查资料，然后切到文档应用，又收到几条来自聊天软件的通知。同时设备电量处于中等水平，网络连接稳定。

Android collector 会通过 UsageStatsManager 记录应用切换，通过 NotificationListenerService 记录通知出现，通过 DeviceContext 记录设备状态。这些事件被写入 append-only JSONL。

Rust daemon 读取 JSONL 后，把事件转换成 RawEvent。RawEvent 进入 Privacy Air-Gap 后，敏感字段会被脱敏或摘要化，形成 SanitizedEvent。WindowAggregator 会把一段时间内的事件聚合成 StructuredContext，比如当前用户处于文档编辑上下文，通知密度较高，设备状态允许轻量预取。

DecisionRouter 接收到 StructuredContext 后，RuleBasedBackend 可能生成几个意图：一个是降低打扰，一个是预取最近相关资源，一个是记录维护建议。如果启用了云端 LLM，它也可能基于脱敏上下文给出更语义化的建议。

这些建议进入 PolicyEngine。PolicyEngine 检查动作目标是否在当前上下文中、风险是否可接受、能力等级是否足够。如果某个动作涉及不明确目标，或者置信度太低，就会被拒绝。通过审查的动作由 ActionLifecycle 形成 AuthorizedAction。

最后，ActionAdapter 执行动作。如果是 Android-safe 的预取动作，可能通过 action bridge 发给 Android collector。如果是本地 fallback，则只记录 trace。无论动作执行、拒绝还是失败，系统都会产生 AuditRecord。

这个例子说明，DiPECS 的智能不是单点发生的，而是贯穿采集、脱敏、聚合、决策、审查、执行和审计的完整链路。

## 十四、局限性

当然，DiPECS 目前仍然是原型系统，也有一些局限。

第一，真实 Android 设备验证还不完整。当前已经有模拟器验证脚本和 Android collector，但真机上的权限授予、trace 导出、adb forward、action bridge 和 APK 安装路径还需要进一步补齐。

第二，系统级高权限采集还没有成为主线实现。Binder/eBPF、fanotify、system image 等路线已经在 spec 和设计中预留，但当前真实 eBPF program loading 和 system image 集成还没有完成。

第三，部分动作仍然以 fallback 或 trace 为主。比如 PreWarmProcess、KeepAlive、ReleaseMemory 等能力，后续需要进一步收敛为 Android-safe 的自有资源动作，避免跨越平台权限边界。

第四，LocalEvaluatorBackend 还需要继续加强。当前规则后端比较稳定，云端 LLM 支持也已经接入，但本地模型评估能力和个性化行为建模还可以继续深入。

第五，真实用户数据和长期效果评估不足。我们已经有 synthetic trace、sample replay 和部分测试场景，但真正衡量 AIOS 价值，还需要长期运行数据，比如动作命中率、用户打断率、资源开销、隐私泄漏风险和用户感知收益。

这些局限并不影响当前原型的价值，反而指出了下一步可以继续扩展的方向。

## 十五、未来工作

后续工作可以从几个方面展开。

第一，完成真机 Android 验证。包括真实设备上的权限配置、采集稳定性、JSONL 导出、action socket 转发、token 管理和真实 trace 样本收集。

第二，增强本地智能能力。可以进一步引入本地小模型或行为预测模型，让系统在不依赖云端的情况下也能学习用户习惯。但这部分仍然要遵守 Privacy Air-Gap 和 AuthorizedAction 边界。

第三，扩展系统侧采集。未来可以在受控环境下接入 fanotify、Binder probe 或 eBPF，获得更丰富的系统事件。但每一种新事件都必须先定义 schema、脱敏规则和策略边界。

第四，完善动作能力。当前 Android-safe 动作子集比较保守，后续可以增加更多自有资源动作，比如更细粒度的缓存管理、应用内预热、用户可见确认式操作等。

第五，强化评估体系。除了功能测试，还可以建立更完整的指标体系，包括端到端延迟、资源开销、动作批准率、动作拒绝率、误触发率、隐私泄漏测试覆盖率、audit hash 稳定性等。

第六，改进人机协同机制。对于风险较高或置信度不足的动作，可以引入用户确认、解释展示和反馈学习，让系统从“自动执行”扩展到“可解释协助”。

## 十六、总结

最后总结一下。

DiPECS 是一个面向 Android/Linux 的本地优先 AIOS 原型系统。它关注的核心问题是，在操作系统层面引入 AI 能力时，怎样既利用模型的上下文理解和决策能力，又不牺牲隐私、安全和可审计性。

为了解决这个问题，我们设计并实现了一条完整链路：Android public API 和 daemon sources 负责采集，Privacy Air-Gap 负责隐私脱敏，WindowAggregator 负责上下文聚合，DecisionRouter 负责生成意图，PolicyEngine 和 ActionLifecycle 负责动作治理，ActionAdapter 和 Android bridge 负责受控执行，Replay 和 Audit 负责验证与追踪。

这个项目的核心思想是：AI 可以参与操作系统决策，但不能绕过操作系统治理。模型可以提出意图，但动作必须由本地策略授权。原始数据可以被系统采集，但不能直接进入模型。动作可以被执行，但必须留下审计记录。

因此，DiPECS 的价值不只在于实现了一个 AIOS demo，更在于提出并实践了一种相对稳健的 AIOS 架构原则：本地优先、隐私隔离、机制策略分离、授权执行和确定性审计。

以上就是 DiPECS 的项目介绍，谢谢大家。

## 可选问答准备

如果被问到“为什么默认不用云端大模型”，可以这样回答：

DiPECS 的定位是本地优先的 AIOS 原型。操作系统上下文包含很多敏感信息，如果默认依赖云端模型，会放大隐私和可用性风险。因此我们让 RuleBasedBackend 作为默认路径，CloudLlmBackend 作为可选增强。这样即使没有网络或 API key，系统也能运行；即使云端失败，也能回落到本地规则或 no-op。

如果被问到“Privacy Air-Gap 和普通脱敏有什么区别”，可以这样回答：

普通脱敏往往只是数据处理步骤，而 DiPECS 把它做成架构边界。RawEvent 不能越过 Privacy Air-Gap，模型后端只能接收 StructuredContext。这意味着隐私保护不是依赖模型自觉，也不是 prompt 里的软约束，而是由类型、模块和数据流共同限制。

如果被问到“为什么模型不能直接执行动作”，可以这样回答：

因为模型输出具有不确定性，而且系统动作可能影响资源、隐私和用户体验。DiPECS 采用机制和策略分离，模型只能输出 IntentBatch，PolicyEngine 才负责审查动作是否符合当前能力等级、风险等级和上下文。AuthorizedAction 只能由 ActionLifecycle 构造，这样可以防止模型绕过本地治理。

如果被问到“项目和普通 Agent 有什么区别”，可以这样回答：

普通 Agent 往往强调完成任务，可能把工具调用和模型决策直接连起来。DiPECS 更强调操作系统式的边界治理，包括隐私隔离、能力分级、动作授权、审计回放和失败降级。它不是一个单纯的聊天 Agent，而是一个面向系统软件的 AIOS 原型管线。

如果被问到“当前最大不足是什么”，可以这样回答：

当前最大不足是真机长期验证还不够。我们已经实现了 Android collector、Rust daemon、replay、audit 和 action bridge，也有模拟器和测试脚本，但真实设备上的长期采集、权限适配、资源开销和用户体验还需要进一步实验。

如果被问到“未来最值得扩展的方向是什么”，可以这样回答：

一个方向是增强本地模型和行为预测能力，让系统更智能但仍然保持本地优先。另一个方向是扩展受控动作能力，让 AuthorizedAction 能覆盖更多实际有用的 Android-safe 操作。同时还需要完善真实设备评估和隐私泄漏测试。
