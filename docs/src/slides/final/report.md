# DiPECS 最终汇报演讲稿（约 40 分钟）

> 建议用法：这份稿子按 40 分钟准备，整体约 8000 字左右。正式讲的时候不要逐字匀速念；图页可以放慢，实验页可以略快。每一节的时间是建议值，现场可以根据老师反应微调。

## 0. 开场与一句话介绍（约 2 分钟）

（第 1 页封面：先停 1 秒，面向老师，不要急着念标题。开场时站在屏幕左侧，让出标题区域。）

各位老师、同学大家好，我们汇报的项目是 DiPECS。我们把它定位为一个面向 Android 和 Linux 的本地优先 AIOS 原型系统。这个系统关注的是一条完整的 OS 控制闭环：本地信号怎样被采集，隐私怎样在模型之前被隔离，模型或规则给出的建议怎样接受授权审查，动作怎样被安全执行，最后又怎样留下可审计、可回放的证据。

（翻到第 2 页“一句话理解 DiPECS”。手指或激光笔从左到右扫过“感知、脱敏、上下文、决策、授权执行、审计”这条链路。这里放慢语速，让听众先建立总印象。）

所以 DiPECS 的核心闭环可以概括为六个词：感知、脱敏、上下文、决策、授权执行、审计。系统先从 Android 应用切换、通知、设备状态，以及 Linux `/proc` 进程状态等本地信号中采集事件；随后在本地通过 Privacy Air Gap 去掉原始通知文本、路径、标识符等敏感内容，只保留结构化提示；接着把若干秒内的事件聚合成上下文窗口；之后由规则、本地评估器，或者可选云模型生成意图；但模型生成的只是候选建议，不能直接操作系统。所有动作必须经过本地策略引擎和生命周期状态机检查，形成 `AuthorizedAction` 后才会交给执行适配器。最后，每个动作都会留下终态审计记录，离线 replay 使用同一套核心逻辑复现结果。

因此，我们的重点放在系统机制上：让 AI 建议进入一个有权限边界、有失败降级、有可回放证据的 OS 控制回路。

（翻到第 3 页“目录”。目录只读六个一级标题，不解释细节。读完后马上翻到下一页，避免在目录页停太久。）

## 1. 问题、目标与定位（约 5 分钟）

（翻到“01 问题、目标与定位”章节页。停顿半秒，提示听众进入第一部分。）

（翻到“从 AI 能力到可信 AIOS 闭环”。先指左边的 AI 能力，再指右边的 OS 闭环，强调我们从模型能力转向系统控制问题。）

先看问题背景。现在大模型越来越多地被放进应用、助手和系统服务里。它可以总结通知、理解用户意图、预测用户接下来可能打开哪个应用，甚至建议提前准备资源。但在操作系统层面，这里有几个关键问题。

第一，本地信号分散在很多 OS 接口里。Android 上有 `UsageStatsManager`、`NotificationListenerService`、前台服务、JobScheduler；Linux 侧有 `/proc`、进程状态、文件访问、Binder 或 fanotify 等可能的系统接口。这些信号来源各自独立，格式、时序和权限边界都不一致，需要系统先把它们整理成统一上下文。

第二，本地信号里有很强的隐私风险。通知正文、聊天联系人、文件路径、通知 key、窗口文本，都可能包含个人信息。如果我们把这些原始内容直接发给云模型，或者直接写进审计日志，就违背了本地优先系统的基本目标。

第三，模型输出不等于系统权限。模型可以建议“预热某个应用”或者“预取某个附件”，但它不应该因为一句自然语言建议就获得系统执行能力。真正的系统动作应该由策略引擎、能力等级、风险边界和授权凭证共同约束。

第四，网络和模型都可能失败。如果关键控制路径依赖云模型，那么网络超时、API 抖动、模型返回格式错误，都会直接影响系统行为。因此即时资源动作必须有本地路径，云端只作为可选分析后端，不进入可信计算基。

基于这些问题，我们给 DiPECS 定位为一个本地优先 AIOS 控制平面原型。我们验证的重点是：系统能否把 AI 建议约束在可控 OS 机制内。具体目标有五点：第一，Android 和 Linux 信号能否形成统一上下文；第二，原始隐私能否在模型前被强制隔离；第三，多级决策能否退化到确定性本地路径；第四，动作能否经过策略、认证和审计后执行；第五，决策器在可观察上下文中能否给出可评测预测。

与此同时，我们也明确当前版本不声称几件事。我们没有修改 Linux 内核、调度器或 Android LMKD；不把合成预测准确率外推成真实用户效果；不保证普通 APK 可以执行所有系统级动作；不把模拟器功耗估算当作真机实测；也不把未接入主链路的本地模型实验包装成最终产品。

（翻到“典型场景”。这一页按流程讲，不要一次性念完。手指从“通知”指到“脱敏提示”，再指到“候选动作”，最后指到“Policy/AuthorizedAction”。）

可以用一个典型场景帮助理解：用户收到聊天软件通知，通知里有附件语义。系统在本地先完成脱敏，只提取“可能有文件”“可能来自某个应用”这样的结构化提示。模型或本地规则可以判断用户可能打开聊天软件，建议预取文件或预热相关资源。但这个建议还要经过策略检查：目标是否在上下文中，风险等级是否可接受，置信度是否足够，动作数量是否超限。如果检查通过，才形成授权动作；如果不通过，就安全地拒绝或 NoOp。

（翻到“项目目标与非目标”。目标读完整，非目标只强调边界，不要显得在道歉。）

（翻到“三项核心贡献”。这里适合做一个小结，手指依次点三块：本地感知、本地优先决策、授权执行与审计。）

因此本项目的三项核心贡献是：第一，本地感知上下文，把通知、应用、进程和设备状态标准化；第二，本地优先决策，用规则、本地评分和可选云模型覆盖不同复杂度，并在敏感、失败或离线时安全降级；第三，授权执行与审计，模型只提出候选，真正执行由策略、能力、HMAC 信封和生命周期共同约束。

## 2. 系统架构与 OS 机制（约 10 分钟）

（翻到“02 系统架构与 OS 机制”章节页。这里提醒听众：下面开始进入实现和 OS 机制。）

接下来讲系统架构。总体上，DiPECS 分为数据面和控制面。数据面负责把本地事件采集、校验、脱敏、聚合成模型可消费的上下文；控制面负责路由决策、策略审查、授权动作、设备执行和审计回放。

（翻到“总体架构”。这页是全场核心图之一，要慢讲。先指最上方 Android 设备侧，再指 Rust 核心数据面，最后指决策与授权控制面。讲的时候按“输入—处理—决策—执行—审计”的顺序移动手指，不要跳着讲。）

从总体架构图看，最上层是本地信号源。Android 侧主要使用公开 API，包括应用前后台切换、通知发布与交互、设备状态等。这些事件写入 app 私有目录下的 append-only JSONL trace。Linux/Rust 侧有 `/proc` 进程状态、系统状态采集，以及可选的 Binder 或 fanotify 特权探针。这里要强调，`actions.jsonl` 的含义是 Android 采集 trace，也就是进入 Rust 管线的事件输入。

中间是 Rust daemon 的数据面。采集器把事件送入 `RustCollectorIngress`，这里做 schema 校验和 SourceTier 标注。之后进入 `PrivacyAirGap`，从 `RawEvent` 转成 `SanitizedEvent`。再进入 `WindowAggregator`，以 10 秒为窗口形成结构化上下文。这个上下文才会给后续决策模块使用。

控制面从 `DecisionRouter` 开始。Router 会根据熔断状态、隐私敏感度、本地可动作信号和语义复杂度选择后端。后端可以是 RuleBased、本地 LocalEvaluator，也可以是可选 Cloud LLM。云模型不进入 TCB，敏感窗口和失败路径都会留在本地。后端输出的是 `IntentBatch`，这一层只产生候选建议；随后 `PolicyEngine` 检查风险、能力、置信度、上下文目标、动作数量等条件。只有通过策略的动作才会进入 `ActionLifecycle`，由生命周期状态机 seal 成 `AuthorizedAction`，再交给 `ActionAdapter` 执行。在线执行可以走 Android Bridge，离线 replay 走 `OfflineAdapter`。

（翻到“部署与进程边界”。手指分三层：Android App、Rust dipecsd、可选模型服务。强调边界，不要在 API 名称上停太久。）

部署上，Android App 进程负责通过公开 API 采集事件，写入私有 JSONL，并暴露 localhost action bridge。Rust `dipecsd` 进程负责 tail JSONL、读取 `/proc`、维护 channel、处理窗口、调用决策和策略。可选模型服务只接收 Sanitized Context，不接收原始文本。

（翻到“模块依赖”。先指顶部 `aios-spec`，再向下指四个核心库，最后指 daemon/CLI。这里强调“协议稳定、入口组合”。）

模块依赖上，我们把稳定协议放在 `aios-spec`，它定义跨模块类型、trait 和 IPC 协议。`aios-collector`、`aios-core`、`aios-agent`、`aios-action` 都依赖 `aios-spec`，但 daemon 和 CLI 只是组合入口，不反向污染核心库。这一点体现了机制和策略分离：采集器和执行器提供机制，Router 和 Policy 决定策略。

（翻到“OS 相关性”。这页按课程视角讲，语速可以快一点；重点点出 daemon、/proc、文件系统、IPC、安全五类机制。）

从 OS 课程角度看，这个项目覆盖了几个典型机制。第一是进程与 daemon。`dipecsd` 使用 `fork + setsid + /dev/null` 完成 daemon 化，主进程退出，后台会话继续运行。第二是 `/proc`。我们通过 `/proc/<pid>/status`、`oom_score` 等接口读取进程 RSS、Swap、线程数和 OOM 分数，并通过快照差分减少冗余事件。第三是文件系统。Android trace 使用 append-only JSONL，Rust tailer 保存 byte offset，并处理半行和文件截断问题。第四是 IPC 和 channel。采集任务和处理任务通过容量 4096 的 mpsc channel 解耦。第五是安全。系统用 capability、policy、HMAC、审计形成最小权限和引用监控器式边界。

（翻到 `/proc` 页。如果图上有字段或表格，指 `status`、`oom_score`、RSS/Swap 这些位置。这里只讲为什么 `/proc` 适合做低侵入观测。）

（翻到“Daemon：进程与会话管理”。先指 daemon 化部分，再指运行时管线图。讲管线时从左边采集任务指到中间 channel，再指到右边处理任务，最后指到底部反馈和退出。）

Daemon 管线图可以分成两条任务看。Task 1 是采集循环。它周期性 poll Android JSONL、ProcReader、SystemState 和可选 BinderProbe。每个事件先进入 `RustCollectorIngress`，贴上来源等级，然后发送到 `raw_events` channel。Task 2 是处理循环。它从 `raw_rx.recv()` 消费事件，经过 `PrivacyAirGap` 脱敏，再推入 10 秒 `WindowAggregator`。窗口关闭后形成 `StructuredContext`，调用 `process_window()`。这个函数内部会从 Memory Store 取近期反馈，调用 Router 和 Lifecycle，最后产出 Audit Records、Runtime Trace，并把结果反馈回记忆。停机时，采集侧 sender drop，处理侧看到 channel closed 后会 flush 剩余窗口，再完成退出。

（翻到“Android OS 服务作为受控事件源”。手指依次点 UsageStats、NotificationListener、前台服务、JobScheduler。Accessibility 只轻点一下，说明它不是主链路。）

Android OS 服务作为事件源时，我们有意识选择公开 API。`UsageStatsManager` 提供应用前后台切换，需要 Usage Access；`NotificationListenerService` 提供通知发布和交互，需要用户显式启用；`AccessibilityService` 只作为可选筛查或调查信号，当前不进入 Rust 主链 RawEvent；前台服务提供轮询和 heartbeat；JobScheduler 用于 KeepAlive 维护任务。选择公开 API 的原因是权限边界清晰、模拟器和真机都可复现，不依赖内核 hook。

（翻到“Append-only JSONL”。指 append-only、byte offset、半行处理这几个关键词。这里可以说“这就是为什么 replay 能稳定复现”。）

JSONL 这部分也值得强调。Android app 每行写一个 CollectorEvent，Rust tailer 只解析包含 Rust-compatible `rawEvent` 的行。没有 `rawEvent` 的调查行不会进入主链。这个设计让同一份 trace 同时服务于在线输入、离线 replay、回归测试和审计取证。

（翻到“Replay / Audit”。手指从在线路径指到离线路径，最后指共同核心逻辑。这里强调“adapter 不同，核心一致”。）

Replay/Audit 的关键思想是在线和离线共用核心逻辑。在线路径经过 AndroidAdapter 执行真实设备动作；离线路径使用 OfflineAdapter，无 I/O、确定性，适合 golden hash。两者只在 adapter 处不同，核心状态迁移、策略审查和审计格式保持一致。

## 3. 隐私、决策与动作治理（约 12 分钟）

（翻到“03 隐私、决策与动作治理”章节页。提醒听众：这一部分是项目核心，要放慢。）

第三部分是隐私、决策和动作治理，这是项目最核心的部分。

（翻到“Privacy Air Gap”。手指先指 RawEvent 原始区，再跨过 Air Gap，指 SanitizedEvent/StructuredContext。强调这是强制边界。）

首先是 Privacy Air Gap。我们把系统分成原始区和安全区。原始区里可以存在 RawEvent，例如通知原文、文件路径、Binder 参数等；安全区之后只能出现 SanitizedEvent、StructuredContext、ModelInput 和 AuditRecord。模型、上下文和审计都只消费 SanitizedEvent。

（翻到“通知文本脱敏”。按两层讲：先指 Android 端提取 hint，再指 Rust 端二次兜底。这里不要展开所有 hint，只举文件、图片、验证码、金融四个例子。）

通知文本脱敏采用双层设计。Android 实采端先在本地从通知标题和文本中提取 `title_hint`、`text_hint` 和 `semantic_hints`，例如长度、脚本类别、是否 emoji、是否包含文件、图片、验证码、金融语义等。提取后，Android 端写入 trace 的 `raw_title` 和 `raw_text` 已经是空字符串。Rust 侧的 PrivacyAirGap 是第二道强制边界，也兼容旧 trace：如果旧 trace 里还有 raw text，Rust 会重新分析并只输出统计量和类别提示。最终原文、路径、通知 key、group key 都不进入模型输入。

（翻到“10 秒上下文窗口”。手指沿时间轴移动，强调多个事件进入同一个窗口；最后指窗口输出的 StructuredContext。）

然后是 10 秒上下文窗口。系统会先在窗口内聚合应用切换、通知、系统状态、进程状态、文件活动等事件，再统一生成 StructuredContext。这个上下文包含窗口起止时间、事件列表、摘要和行为特征。这样做有两个好处：第一，减少模型调用频率；第二，把瞬时事件变成上下文，让策略可以判断目标是否真的出现在当前窗口里。

（翻到“DecisionRouter”。这页按优先级从上到下讲：熔断、隐私敏感、本地信号、语义复杂度。讲到 Cloud LLM 时停一下，强调它是可选路径。）

决策路由由 `DecisionRouter` 控制。它的优先级是：先看熔断状态，如果最近连续错误超过阈值，直接进入 FallbackNoOp；再看隐私敏感度，如果验证码或金融语义太多，阻止云端路径；再看本地可动作信号，比如文件访问、低电量、屏幕交互，这些直接走本地评估；最后才根据语义复杂度选择 RuleBased、LocalEvaluator 或 Cloud LLM。云端后端如果配置错误或调用失败，会退回本地规则，并把错误写入结果。成功后错误计数清零。

（翻到“LocalEvaluator”。手指点特征输入、加权评分、结构化输出。这里用一句话解释它为什么可解释。）

LocalEvaluator 是一个可解释的确定性评分器。它会把候选动作按若干显式特征加权，例如上下文里是否出现目标应用，是否有文件或图片语义，是否有近期执行失败，是否有低电量或系统压力信号。输出受到约束：目标、动作类型、置信度、风险等级都结构化。这样策略引擎可以继续审查。

（翻到“PolicyEngine”。这页逐项指 8 个检查框，不需要展开每个实现细节；重点强调顺序执行、fail closed、拒绝有原因。）

接下来是 `PolicyEngine`。这里我们强调一句话：模型建议必须经过授权。PolicyEngine 按顺序 fail closed。它检查后端能力等级、动作风险上限、置信度下限、batch 上限、阻止列表、延迟动作紧急度、动作能力白名单，以及目标是否在上下文中。只有全部通过，才会产生 `PolicyActionDecision::Approved`。否则每个拒绝都有明确 `DenialReason`。

（翻到“ActionLifecycle”。手指沿状态机走一遍：校验、Policy、seal、adapter、终态。讲到终态时强调“每个动作恰好一个终态”。）

ActionLifecycle 则保证每个动作恰好一个终态。它会先做 schema 校验，再调用 PolicyEngine，再 seal AuthorizedAction，再调用 adapter。如果 adapter 成功，记录 Succeeded；如果连接失败、超时、设备拒绝、非法回执，都会进入 Failed；如果策略拒绝，则进入 DeniedByPolicy 或相关终态。只有 lifecycle 能构造 AuthorizedAction，执行层不能自己伪造。

（翻到“Android Action Bridge”。先指信封字段，再指 HMAC 覆盖范围，最后指 60 秒 freshness。这里要讲得清楚，因为这是安全边界。）

Android Action Bridge 负责把授权动作送到设备侧。信封包含 `message_type = execute`、`issued_at_ms`、`expires_at_ms = issued + 60s`、canonical AuthorizedAction JSON，以及 `auth.hmac_sha256`。HMAC 覆盖 protocol version、issued time、expires time、action length 和 action bytes。所以旧标签不能替换动作，也不能跨过 freshness window 使用。这里我们也诚实说明：当前原型没有实现 nonce 级 replay cache，因此当前能力边界是认证和时效窗口约束，完整防重放系统还需要后续补齐。

（翻到“动作与权限边界”。手指依次点 PreWarm、Prefetch、KeepAlive、ReleaseMemory、NoOp。这里不要承诺收益，只讲动作语义和权限边界。）

动作与权限边界也很重要。`PreWarmProcess` 可以启动目标 Activity，普通 APK 对第三方后台启动会受限；`PrefetchFile` 可以预取 HTTPS 或 content URI 到受控缓存；`KeepAlive` 尝试 OOM/cgroup 并 fallback 到 JobScheduler；`ReleaseMemory` 清理预取缓存，特权环境下可以尝试包缓存或 page cache；`NoOp` 是确定性安全退化路径。同一动作在普通 APK 环境可能退化、被拒绝或只作用于自身资源，这些结果都会在审计中明确体现。

## 4. 闭环运行证据（约 4 分钟）

（翻到“04 闭环运行证据”章节页。这里告诉听众：下面从设计转到证据。）

第四部分讲系统跑通证据。我们准备了三类证据：输入链路、策略裁决、设备执行。

（翻到“案例输入”。手指从 Android 公开 API 指到 JSONL，再指到 Rust replay/audit 和 hash。这里强调链路闭合。）

首先是本地事件进入系统。模拟器实采 smoke E2E 验证了 Android 35 公开 API 事件能写入 JSONL，被 Rust replay/audit 读取，并生成稳定审计哈希。这个证据的作用是证明 Android 事件到 Rust 管线、再到审计 hash 的路径已经闭合。

（翻到“案例裁决”。左边讲允许，右边讲拒绝。手指指向拒绝原因时放慢，强调 Policy 独立于模型。）

其次是案例裁决。我们展示了允许和拒绝场景：即使模型或本地后端给出高置信度建议，也不能绕过上下文事实。如果目标不在当前窗口、能力等级不够、风险超过上限，Policy 就会拒绝。也就是说，Policy 是独立于模型的硬边界。

（翻到“设备执行证据”。指四类处理器的终态记录。这里按四个动作名称快速扫一遍，不逐条展开日志。）

第三是设备执行证据。我们验证了四类可转发动作处理器都有终态。KeepAlive 记录 `keep_alive_scheduled` 到 `job_executed`；ReleaseMemory 记录 `release_memory_completed`；PreWarmProcess 记录 `own_resources_prewarmed`；PrefetchFile 记录 `prefetch_started` 到 `prefetch_succeeded`。设备侧记录 `authorized_action_socket_execute_ok = 4`，四类处理器均留下终态。

这里也要说明诚实边界：PreWarm 验证的是 `own:warmup` 处理器链路，不证明第三方应用预热收益；Prefetch 回执只表示入队，最终下载由 `prefetch_succeeded` 取证。设备证据通过与生产信封逐字节一致的取证发送器经 adb forward 获得，证明设备处理器执行，不等价于 daemon 已经在设备内完整生产部署。Rust AndroidAdapter 另有 mock-socket E2E 测试。

## 5. 实验设计、结果与边界（约 7 分钟）

（翻到“05 实验设计、结果与边界”章节页。这里语速可以略快，但每个结论都要带证据等级。）

第五部分是实验结果。我们把证据分为真实 API、模拟器实测、离线 replay 和估算值，不把不同强度的结论混在一起。

（翻到“实验问题与证据层级”。先指左侧问题，再指右侧证据类型。告诉听众：我们把强证据和弱证据分开。）

第一个问题是：管线能否从 Android 事件走到可复现审计？证据是模拟器实采 smoke E2E 加 replay hash。第二个问题是隐私边界是否阻止原始通知泄漏，证据是 naive prompt 与 DiPECS 输入对照。第三个问题是本地决策为何比云端适合即时路径，证据是规则、本地和真实 API 延迟。第四个问题是当前决策器能否预测上下文支持的下一应用，证据是合成 trace 和派生 ground truth。第五个问题是常驻开销是否可控，证据是 emulator CPU/RSS/PSS 和离线 replay 吞吐。第六个问题是动作治理是否覆盖主要拒绝路径，证据是 Policy 测试和 action audit。

（翻到“合成预测评测”。先指数据范围：946、764、178；再指 Top-1/Top-3。最后必须指 eligible coverage 和错误率，避免听起来像泛化结论。）

先看合成预测评测。数据来自三个确定性合成场景，使用 10 秒上下文窗口和 30 秒预测 horizon。总窗口 946，有未来切换 764，其中上下文可支持标签 178，占 23.3%。在这些 eligible 窗口上，RuleBased 的 Top-1 是 61.2%，Top-3 是 65.7%，预测覆盖 93.8%；LocalEvaluator 的 Top-1 是 43.8%，Top-3 是 62.9%，预测覆盖 73.6%。这个结果说明规则在合成、上下文可观察场景里能捕捉一部分可解释模式。但我们不把它外推到真实用户泛化准确率，RuleBased 的条件错误预测率仍然有 34.7%。

（翻到“决策延迟”。手指先点本地两行，再点 Cloud LLM 行。讲云端 7 到 10 秒时停顿一下，让对比更明显。）

再看决策延迟。RuleBased p50 约 0.00 ms，p95 0.02 ms；LocalEvaluator p50 0.01 ms，p95 0.05 ms；Cloud LLM 使用 2026-07-01 的 10 轮真实 API 数据，p50 约 7.34 秒，p95 约 10.05 秒。结论很明确：即时资源动作默认应该本地完成；云端只适合作为可选分析路径，不能成为关键控制回路依赖。这个数据只证明一次真实 API 延迟量级，不证明云端决策质量或稳定收益。

（翻到“隐私与治理结果”。先指 22 到 0 的泄漏对比，再指输入大小下降，最后指 PolicyEngine 20 项测试。）

隐私对照实验中，naive cloud prompt 会把原始通知文本直接拼进输入，检测到 22 个原始文本泄漏；DiPECS 路径为 0。输入大小从 63,178 bytes 降到 645 bytes。审计流与 NDJSON 泄漏测试 2/2 通过。这说明 Privacy Air Gap 不只是减少数据量，也是在模型和审计前建立强制边界。

治理覆盖方面，PolicyEngine 20 项测试通过，覆盖 target-in-context、risk/capability 双重上限、confidence floor、batch cap、deferred filter 和 FallbackNoOp 能力隔离。它验证的是：即使后端提出动作，策略仍能作为第二道审查防线。

（翻到“资源开销与离线吞吐”。指 PSS 增量，再指 replay 吞吐。CPU 和电池估算一句带过，不要作为收益讲。）

资源开销方面，Android emulator 每个模式 30 个样本。baseline PSS 是 36.024 MB，observe only 是 39.629 MB，增加 3.605 MB；action loop 是 41.621 MB，增加 5.597 MB。离线 2400-line replay 中，有效事件 1631，完成窗口 58，wall time 128 ms，peak RSS 10.77 MB，吞吐 12,742 events/s，授权动作 206。CPU 采样有粒度噪声，电池和温度是模拟器估算，所以不作为真机功耗结论。

（翻到“负面结果与有效性威胁”。这页要主动讲，语气平稳。手指按四类负面结果依次点，最后落到“后续重测/真机验证”。）

最后是负面结果和有效性威胁。当前数据不支持“预热快 43.8%”的因果结论，因为脚本混入了 cold/warm process 差异；不支持 ReleaseMemory 有效降低 PSS，结果反而增加 0.331 MB；不支持模拟器电池/温度代表真机功耗；不支持 Cloud LLM 能稳定产生有效即时动作；也不支持合成准确率代表真实用户行为。因此我们的表述是：启动时间数据不进入正面结论，待独立 controller 重测；ReleaseMemory 只视为链路覆盖，不视为收益证明；功耗结果标为 estimated；云端视为非即时可选后端；预测结果同时报告 eligible coverage 和错误预测率。

## 6. 局限、未来工作与总结（约 2 分钟）

（翻到“06 局限、未来工作与总结”章节页。这里准备收束，不再引入新概念。）

（翻到“项目边界”。左边讲已实现，右边讲未深入。不要把未深入讲成缺陷，讲成原型边界。）

最后总结项目边界。DiPECS 当前是用户态控制平面。已实现的是：使用 OS 暴露的进程、文件、IPC 和服务接口；在用户态实现机制和策略分离；建立隐私、能力、策略、认证和审计边界；在模拟器跑通设备动作处理器与授权回路。未深入的是：没有修改 scheduler、LMKD、VFS 或 Binder driver；普通 APK 受 Android 沙箱和后台启动限制；Binder/fanotify 探针仍依赖特权部署；系统级动作需要 platform signing 或 ROM 集成。

（翻到“下一步”。按评测、OS 集成、决策三类讲，每类一句即可。）

下一步工作分三类。评测上，需要构建真实用户、匿名化 ground truth 数据集，重做启动实验，测真机功耗和长期稳定性。OS 集成上，可以接入 LMKD/cgroup 反馈，使用 Binder 或 Unix domain socket，做 platform-signed system app。决策上，可以接入端侧轻量模型，与规则基线对照，并加入资源预算和预热撤销机制。

（翻到“总结”。回到六个词：本地感知、隐私边界、决策路由、授权执行、可回放审计。最后一句“谢谢大家”说完停顿，不要立刻切 Q&A。）

最后用一句话总结：DiPECS 探索的是智能操作系统中“本地感知、隐私边界、决策路由、授权执行、可回放审计”的闭环机制。它把模型建议放进 OS 熟悉的权限、策略、生命周期和审计框架里。这个原型说明，AIOS 的关键不只在于 AI 能力，还在于系统如何限制、验证和追责这些能力。谢谢大家。

## Q&A 备用回答提示

（翻到 Q&A 页后不要主动展开所有问题。老师问到哪类问题，再从下面挑相应回答。回答时先给一句结论，再补一句证据或边界。）

如果老师问为什么不用纯规则：纯规则可解释、低延迟，但覆盖复杂语义有限；DiPECS 保留规则作为安全底座，同时允许本地评估器和可选云后端扩展复杂场景。

如果老师问为什么不用纯云端：云端延迟高、隐私风险高、网络不稳定，不能成为即时控制回路的依赖。DiPECS 把云端放在非 TCB 的可选路径。

如果老师问是不是系统级 OS：当前是用户态控制平面原型，不修改内核，但使用了 OS 暴露的进程、文件、IPC、权限和服务机制，目标是验证智能 OS 控制闭环的设计原则。

如果老师问 replay 的意义：replay 让我们不用依赖现场 Demo，也能复现同一组输入下的策略、审计和 hash，从而发现隐私泄漏或策略回归。

如果老师问动作是否真的执行：四类可转发动作处理器在模拟器设备侧均有终态审计事件；但第三方应用预热收益、真机功耗和长期稳定性仍是后续工作。
