# RFC-0001: 分层采集与决策路由重构

## 摘要 (Summary)

本 RFC 提议将 DiPECS 重构为以 `aios-spec` 为协议中心、以 collector 作为 apps 与 Rust 管道之间的采集接口、以 Rust 侧输出结构化数据、以 `aios-core` 为隐私与策略审查边界、以决策路由层选择 rule-based / 本地小模型 / 云端大模型、最终由 action 执行安全动作的分层管线。

## 动机 (Motivation)

当前系统已经形成 `RawEvent -> PrivacyAirGap -> StructuredContext -> MockCloudProxy -> PolicyEngine -> ActionExecutor` 的最小闭环，但 Android 采集、系统级采集、云端推理和本地规则之间的边界仍容易混在一起。继续扩展前需要先明确三件事：

1. `aios-spec` 必须成为跨模块数据结构、协议格式和能力声明的唯一事实来源。
2. Android 采集需要先在 App 用户态验证数据源，再通过 collector 接口进入 Rust；后续可以逐步下沉到 daemon / system 层，而不是一开始绑定高权限路径。
3. apps 不负责生产系统最终使用的结构化上下文；Rust 侧负责把入口事件规范化、脱敏、聚合，并输出 `SanitizedEvent` / `StructuredContext`。
4. 推理与动作之间必须有两道本地防线：数据出推理层前由 `core` 脱敏，动作出执行层前由 `core` 审查。

该重构的目标不是一次性替换现有实现，而是给后续模块扩展提供稳定边界：apps 可以替换采集来源，collector 保持 apps 与 Rust 的入口协议稳定，agent 可以替换推理后端，action 可以替换执行能力，但数据契约和审查语义保持稳定。

## 设计评估 (Evaluation)

整体方向合理，且与现有“机制-策略分离”原则一致。将 `spec` 放在最底层可以减少跨组并行开发时的接口漂移；先从 Android App 用户态采集开始，也符合当前 `apps/android-collector` 的定位，能在真实权限边界内筛选 `UsageStatsManager`、`NotificationListenerService`、`AccessibilityService` 和设备状态等数据源。

需要收紧的地方有三点：

- “接口”需要拆成更明确的 `DecisionRouter` / `AgentGateway`。它不应该直接接收原始数据，也不应该直接发动作；它只接收 `StructuredContext`，输出符合 `aios-spec` 的 `IntentBatch`。
- 本地小模型只能做条件评估、路由选择或输出低风险意图候选。无论它选择 rule-based 还是云端大模型，结果都必须回到 `core` 的 `PolicyEngine` 复审。
- collector 接口不是 Android App 本体，也不是独立部署的中间进程。它是 **Rust 侧的入口抽象**（trait + `CollectorEnvelope` 协议）：当采集来源是 Android App 时，它通过 JSONL、JNI 或本地 socket 接收 apps 侧原始观测；当采集来源下沉到 Rust 系统采集器时，它直接接收同源的 `RawEvent`。无论哪种来源，collector 接口只负责：入口协议校验（schema 版本、来源能力等级声明）、传输批次边界、错误上报。脱敏、聚合、推理选择和动作授权都不放在 apps 或 collector 接口内，而是交给 `aios-core` 和 `aios-agent`。

因此，本 RFC 建议采用“同一入口协议，Rust 统一结构化；同一意图格式，多后端推理；同一动作出口，统一审查”的重构路线。

## 设计方案 (Design)

### 1. 分层职责

```text
aios-spec
    定义 RawEvent、SanitizedEvent、StructuredContext、IntentBatch、
    SuggestedAction、策略结果、能力等级和序列化协议。

apps
    通过 Android 用户态或系统态能力采集原始观测。
    第一阶段由 apps/android-collector 筛选可用数据源。

collector (Rust 侧入口 trait / CollectorEnvelope 协议)
    不是独立进程，而是 Rust 侧的入口抽象。
    当来源为 Android App 时对接 JSONL/JNI/socket；
    当来源为 Rust 系统采集器时直接接收 RawEvent。
    负责入口协议校验、传输批次边界、schema 版本和错误上报；
    不负责最终结构化上下文，也不负责推理或动作策略。

aios-collector (Rust 采集层入口)
    将 app 侧或 system 侧采集输入规范化为 CollectorEnvelope / RawEvent。

aios-core
    接收 RawEvent, 输出 RawEvent -> PrivacyAirGap -> SanitizedEvent -> WindowAggregator -> StructuredContext。

aios-core
    出站：IntentBatch -> PolicyEngine -> AuthorizedAction / DeniedAction。
    core 是隐私脱敏边界和动作审查边界。

aios-agent / DecisionRouter
    接收 StructuredContext，选择 rule-based、本地小模型或云端大模型。
    所有后端都返回同一类 IntentBatch。

action / Android API
    只执行 core 审查通过的动作，并把执行结果写入 Trace。
```

运行时主链路保持单向流动：

```text
apps/android-collector (Phase 1-2) 或 aios-collector (Phase 3+)
    -> collector interface (Rust 侧入口协议, 同一套 CollectorEnvelope)
    -> aios-collector (collector ingress)
    -> aios-core (PrivacyAirGap → WindowAggregator)
    -> aios-agent (DecisionRouter)
    -> aios-core (PolicyEngine)
    -> aios-action / action executor
```

代码依赖仍以 `aios-spec` 为最低层：apps、collector、core、agent、action 都只能依赖 `aios-spec` 的协议类型，不能让 `aios-spec` 依赖任何业务模块，也不能让 action 层反向读取 apps、collector 或 agent 内部状态。

### 2. 数据流

生产路径如下：

```text
Android API / system source
    -> apps
    -> collector interface
    -> aios-collector ingress
    -> RawEvent
    -> PrivacyAirGap
    -> SanitizedEvent
    -> WindowAggregator
    -> StructuredContext
    -> DecisionRouter
       -> RuleBasedBackend | LocalEvaluatorBackend | CloudLlmBackend
    -> IntentBatch
    -> PolicyEngine
    -> AuthorizedAction
    -> ActionExecutor
    -> ExecutedAction / Trace
```

其中 apps 侧可以保留用于调试的原始观测，但生产管线中的 `RawEvent` 由 `aios-collector` 规范化产生，`SanitizedEvent` 和 `StructuredContext` 由 `aios-core` 产生。`RawEvent` 只允许存在于 collector 到 `PrivacyAirGap` 的短路径上。`StructuredContext` 是推理层唯一输入，`IntentBatch` 是推理层唯一输出，`AuthorizedAction` 是 action 层唯一可执行输入。

### 3. 协议层 (`aios-spec`)

`aios-spec` 负责定义协议和数据结构，不包含平台 API 调用、脱敏规则、模型路由逻辑或执行逻辑。

建议逐步补齐以下协议对象：

- `SourceTier`：区分 `PublicApi`、`PrivilegedDaemon`、`SystemImage` 等采集能力等级。
- `CollectorEnvelope`：包装 apps 侧输入或 Rust 侧 `RawEvent`，携带 schema 版本、采集源、时间戳、设备侧 trace id 和采集能力声明。
- `DecisionRoute`：记录本次窗口选择了 `RuleBased`、`LocalEvaluator`、`CloudLlm` 还是 `FallbackNoOp`。
- `DecisionBackendResult`：统一后端返回，包含 `IntentBatch`、路由原因、耗时、置信度和错误信息。
- `AuthorizedAction`：将 `PolicyEngine` 审查通过的动作与原始 `SuggestedAction` 区分开，避免 action 层误执行未经授权的建议。
- `CapabilityLevel`：与 `DecisionRoute` 绑定，声明每个后端能产出的最大风险等级和允许的动作类型。`PolicyEngine` 据此在结构层面拦截越权意图。

这些类型可以分阶段加入。第一阶段可以继续复用现有 `RawEvent`、`StructuredContext`、`IntentBatch` 和 `SuggestedAction`，但 RFC 要求新增类型时必须先更新 `aios-spec`，再修改 apps、collector、core、agent 或 action。

### 4. Android 采集路线

采集按能力等级渐进推进：

| 阶段 | 采集位置 | 主要来源 | Rust 输出 | 目标 |
| --- | --- | --- | --- | --- |
| Phase 1 | Android App 用户态 | UsageStats、NotificationListener、DeviceContext、可选 Accessibility | JSONL trace + Rust 可解析入口样本 | 验证公开 API 能看到什么 |
| Phase 2a | Android public API production bridge | 已提升的 UsageStats、NotificationListener、DeviceContext | append-only `rawEvent` JSONL -> `CollectorEnvelope` | 生产接入 Rust daemon/core |
| Phase 2 | App-Rust collector 接口 | JSONL file tailing | `RawEvent` stream | Complete for v0.2; JNI/local-socket direct ingress is explicitly deferred, not required for this release |
| Phase 3 | daemon / collector | `/proc`、Binder probe、system collector | `RawEvent` stream | 增强系统态观测能力 |
| Phase 4 | system image / privileged service | 更稳定的系统服务接口 | `RawEvent` stream | 降低采集延迟与权限摩擦 |

第一阶段不把 root、Shizuku、eBPF、fanotify 或 system image 作为必需条件。它们只能作为后续增强路径，并且必须复用同一套 `RawEvent` / `CollectorEnvelope`。

### 5. 决策路由

`DecisionRouter` 的职责是根据已脱敏上下文选择最小足够的推理后端：

- `RuleBasedBackend`：处理确定性、低风险、高频规则。例如低电量释放非关键内存、屏幕亮起保持前台进程、明显的 NoOp。
- `LocalEvaluatorBackend`：本地小模型或轻量分类器，只处理是否需要升级到云端、候选意图排序、低风险预判等任务。
- `CloudLlmBackend`：处理需要更强语义理解、跨事件关联或不确定性较高的窗口。
- `FallbackNoOp`：网络不可用、模型超时、上下文不足或隐私预算不足时返回保守 NoOp。

#### 路由决策流程

`DecisionRouter` 采用优先级从高到低的决策链。当前面的条件触发时，跳过后续判断：

```text
1. 熔断 / 强制降级
   ├─ 全局熔断器打开 (连续超时 N 次 或 错误率超过阈值)
   │  └─ 路由到 FallbackNoOp (返回 Idle + NoOp)
   ├─ 网络不可用 且 窗口无复杂语义标签
   │  └─ 路由到 RuleBasedBackend
   └─ 否则继续判断

2. 隐私预算约束
   ├─ source_tier == PublicApi && summary 不含敏感语义标签
   │  → 可以路由到云端 (数据已脱敏, 不暴露原文)
   ├─ source_tier == Daemon && 窗口包含 FinancialContext / VerificationCode
   │  → 禁止路由到云端 (即使已脱敏, 守护进程级数据更敏感)
   │  └─ 路由到 RuleBasedBackend 或 LocalEvaluatorBackend
   └─ 否则继续判断

3. 语义复杂度评估
   ├─ 窗口只有 Screen / SystemStatus / 简单的 AppTransition
   │  └─ 路由到 RuleBasedBackend (确定性规则即可覆盖)
   ├─ 窗口包含 InterAppInteraction + Notification + 多 app 切换
   │  └─ 路由到 CloudLlmBackend (跨事件关联需要语义理解)
   ├─ 窗口含 FileMention / CalendarInvitation / UserMentioned
   │  └─ 路由到 CloudLlmBackend (需要理解用户意图)
   └─ 不确定性窗口 → 路由到 LocalEvaluatorBackend 做预判, 再决定是否升级

4. 动作风险上限
   ├─ 历史窗口中该 app/场景 的最高风险动作 > Medium
   │  └─ 即使匹配简单规则, 也升级到 CloudLlmBackend
   └─ 否则采用步骤 3 的决策
```

路由选择结果写入 `DecisionRoute`，进入 Trace。复盘时可以回答”为什么这个窗口走了规则/本地/云端”。

#### 后端能力等级 (CapabilityLevel)

为防止本地后端被误用为高权限决策器，每个后端声明自己的能力上限。`PolicyEngine` 在审查时直接拒绝超出后端能力的意图——这是结构层面的约束，而非约定：

| 后端 | 最大风险等级 | 允许的动作类型 | 说明 |
| :--- | :--- | :--- | :--- |
| `RuleBasedBackend` | `Low` | `NoOp`, `ReleaseMemory`, `KeepAlive` | 仅确定性、无副作用动作 |
| `LocalEvaluatorBackend` | `Low` | `NoOp`, `PreWarmProcess`, `PrefetchFile`, `ReleaseMemory`, `KeepAlive` | 可做预取/预热, 不可做写操作 |
| `CloudLlmBackend` | `Medium` | 所有 `ActionType` | 高风险动作仍需 PolicyEngine 逐条审查 |
| `FallbackNoOp` | `Low` | `NoOp` | 仅返回 Idle + NoOp, 不执行任何动作 |

`CapabilityLevel` 在 `aios-spec` 中定义，与 `DecisionRoute` 绑定。`PolicyEngine` 审查时校验两条：

1. 意图的 `RiskLevel` 是否 ≤ 此后端的最大风险等级
2. 每个 `SuggestedAction.action_type` 是否在后端允许的动作白名单内

违反任一条 → 意图被拒绝，原因写入 `PolicyDecision.rejection_reason`。

所有后端必须输出 `IntentBatch`。路由选择本身也进入 Trace，便于复盘”为什么这个窗口走了规则/本地/云端”。

### 6. Core 双重审查

`aios-core` 维护两条不可绕过的防线：

1. **隐私防线**：所有 `RawEvent` 必须先经过 `PrivacyAirGap`。通知标题、正文、UI 文本、文件路径、联系人等敏感字段不得出现在 `SanitizedEvent`、`StructuredContext`、模型请求或上传载荷中。
2. **动作防线**：所有 `IntentBatch` 必须经过 `PolicyEngine`。`SuggestedAction` 只是建议，只有审查通过后才能变成 `AuthorizedAction` 并交给 action executor。

这意味着即使推理后端是本地规则，也不能直接执行动作；即使推理后端是本地小模型，也不能接触原始敏感数据；即使云端返回了高置信度动作，也必须接受本地策略复审。

### 7. Trace 与可观测性

Trace 应覆盖整条链路：

```text
CollectorEnvelope
    -> RawEvent stats
    -> SanitizedEvent
    -> StructuredContext
    -> DecisionRoute
    -> IntentBatch
    -> PolicyDecision
    -> ExecutedAction
```

本地开发可以保留原始 trace 以便调试，但默认导出和回归数据应优先使用脱敏后的 trace。Golden Trace 至少验证三类性质：

- 同一 `RawEvent` 输入产生确定的 `SanitizedEvent`。
- 同一 `StructuredContext` 在固定后端配置下产生确定的路由和意图。
- 同一 `IntentBatch` 在固定策略配置下产生确定的授权动作或拒绝原因。

### 8. 全链路错误处理与降级策略

管线中每个阶段都可能失败。错误处理不是各模块自行决定，而是遵循统一的降级链。

#### 错误分类

| 类别 | 示例 | 策略 |
| :--- | :--- | :--- |
| **可恢复** | 云端超时、网络抖动、单条事件解析失败 | 降级到次优后端 / 跳过该事件 / 重试 |
| **不可恢复** | 通道关闭（处理端退出）、策略配置损坏 | 安全关闭，写出已处理结果 |

#### 各阶段错误处理

```text
1. Collector Ingress (schema 校验)
   ├─ 不支持的 schema 版本 → 记录错误，丢弃该 envelope
   ├─ envelope 格式损坏 → 记录错误，丢弃该 envelope
   └─ 通过 → 提取 RawEvent，进入脱敏

2. PrivacyAirGap (脱敏)
   ├─ 无法识别的 RawEvent 变体 → 记录 warning，跳过（不进入 SanitizedEvent 流）
   ├─ 脱敏逻辑本身应无失败路径（纯函数，只做类型转换+文本分析）
   └─ 输出 SanitizedEvent → 进入窗口聚合

3. WindowAggregator (窗口聚合)
   ├─ 窗口为空 → 不产生 StructuredContext，静默跳过
   └─ 窗口到期 → 关闭窗口，产生 StructuredContext

4. DecisionRouter (决策路由)
   ├─ 所有后端不可用，或网络不通且无本地后端
   │  └─ 路由到 FallbackNoOp（返回 Idle + NoOp，保证管线不阻塞）
   ├─ 单个后端超时
   │  └─ 降级到次优后端（CloudLlm → LocalEvaluator → RuleBased → FallbackNoOp）
   ├─ 全局熔断器打开
   │  └─ 跳过云端，降级到 RuleBasedBackend
   └─ 输出 DecisionBackendResult (必须非空，至少含一个 NoOp 意图)

5. PolicyEngine (策略审查)
   ├─ 意图被拒绝 → 记录 PolicyDecision.rejection_reason，跳过执行
   ├─ 策略配置无效 → 使用保守默认值（max_auto_risk=Low, 禁止 ReleaseMemory）
   └─ 输出 AuthorizedAction[] 或空列表

6. ActionExecutor (动作执行)
   ├─ 单个动作执行失败 → 记录 ActionResult{success=false, error=..}，继续执行后续动作
   ├─ 动作类型未实现 → 返回失败，不 panic
   └─ 写入 Trace
```

#### 降级链

系统在任何情况下都不能不产生输出。降级优先级从最优到兜底：

```text
CloudLlmBackend → LocalEvaluatorBackend → RuleBasedBackend → FallbackNoOp
     (最优)              (降级 1)              (降级 2)           (兜底)
```

`FallbackNoOp` 返回的内容固定为：一个 `IntentType::Idle` + `RiskLevel::Low` + `ActionType::NoOp` + `ActionUrgency::IdleTime`。这确保管线即使在全降级状态下仍能产生合法 Trace，`dipecsd` 不会阻塞或 panic。

#### 错误可观测性

所有错误（envelope 丢弃、脱敏跳过、后端超时、动作失败）必须写入 Trace，携带：

- 发生阶段（ingress / airgap / router / policy / executor）
- 错误类型标签
- 时间戳

这些 Trace 不包含原始敏感数据。Golden Trace 回归验证时，**同一类错误应产生同一类降级结果**，保证降级路径也是确定性的。

## 影响面 (Impact)

- 涉及的模块：
  - `crates/aios-spec/`：新增或稳定 envelope、decision route、authorized action 等协议类型。
  - `apps/android-collector/`：继续作为 Android 公共 API 采集端，保留 app 侧 trace preview；已提升数据源通过 `rawEvent` JSONL 进入生产 Rust ingress，未建模数据源才继续作为接口筛选工具。
  - App-Rust collector 接口：负责把 apps 侧事件交给 `aios-collector`。v0.2 决议：以 append-only JSONL tailing 作为完成路径；JNI 或本地 socket 直连只在后续出现低延迟生产需求时重新立项。
  - `crates/aios-core/`：强化 `PrivacyAirGap`、`WindowAggregator`、`PolicyEngine` 的边界语义。
  - `crates/aios-agent/`：从单一 `MockCloudProxy` 演进为 `DecisionRouter` + 多后端。
  - `crates/aios-action/` / action executor：只接收授权动作，并记录执行结果。
- 接口变更：
  - 短期保持现有 `RawEvent`、`StructuredContext`、`IntentBatch` 可用。
  - 中期新增 `CollectorEnvelope`、`DecisionRoute`、`DecisionBackendResult`、`AuthorizedAction`、`CapabilityLevel`。
  - apps 直接上传原始事件的路径仅用于筛选和调试，不作为生产推理路径。
- 向后兼容性：
  - 现有 JSONL `rawEvent` 外部标签格式是 Android 公共 API 到 Rust 的生产入口；`rawEvent: null` 的筛选行不会进入生产推理路径。
  - 现有 `MockCloudProxy::evaluate(StructuredContext) -> IntentBatch` 可作为 `RuleBasedBackend` 或 `MockBackend` 迁移。
  - 未实现系统态采集时，App 用户态采集仍能跑通闭环。

## 风险与缓解 (Risks)

- **边界过度抽象**：过早引入太多 envelope 和 backend 类型会拖慢 MVP。缓解方式是先保留现有类型，只把新类型作为协议演进目标。
- **本地小模型职责膨胀**：小模型可能被误用为动作决策器。缓解方式：(1) 通过 `CapabilityLevel` 在结构层面限制每个后端能产出的最大风险等级和允许的动作类型，`PolicyEngine` 审查时直接拒绝越权意图；(2) 本地后端只能输出 `Low` 风险意图，不能输出 `Medium`/`High`。
- **原始数据泄漏**：apps 调试链路可能绕过 core。缓解方式是生产路径禁止 apps 或 collector 接口直接上传原始事件；可供模型使用的数据必须由 Rust 侧经过 `PrivacyAirGap` 后产生。
- **系统态下沉带来权限成本**：daemon、system image 和 eBPF 路线部署复杂。缓解方式是保持 Phase 1 用户态采集可独立运行，系统态采集只作为能力增强。

## 替代方案 (Alternatives)

### 方案 A：apps / collector 接口直接对接云端

apps 采集后通过 collector 接口直接上传给云端模型，由云端返回动作。该方案实现快，但隐私边界弱，且绕过 Rust 侧结构化输出和 `core` 的策略审查，不符合 DiPECS 的机制-策略分离原则。因此不采用。

### 方案 B：全部使用云端大模型

所有窗口都发送到云端推理。该方案语义能力强，但延迟、成本、网络依赖和隐私压力都更高，也不适合低风险高频动作。因此不采用。

### 方案 C：全部使用本地规则或本地小模型

所有判断都在本地完成。该方案隐私和延迟更好，但复杂语义理解能力不足，难以利用云端大模型和 skills 的泛化能力。因此不采用。

### 方案 D：先做系统态采集，再补 App 用户态采集

先实现 daemon / system image / eBPF 路线可以获得更底层信号，但工程风险和权限成本高，也不利于快速验证 Android 公开 API 的真实可观测性。因此不采用。

## 迁移计划 (Migration Plan)

1. 固定 `aios-spec::RawEvent`、`StructuredContext`、`IntentBatch` 的当前序列化格式。
2. 将 `apps/android-collector` 的 JSONL `rawEvent` 作为 Android 公共 API 的生产入口；没有 Rust schema 的来源继续作为筛选材料。
3. 定义 `aios-collector` 为 Rust 侧采集层入口：接收 `CollectorEnvelope`（无论来自 apps 的 JSONL/JNI/socket 还是 Rust 系统采集器），输出经 schema 校验的 `RawEvent`。
4. 在 `aios-core` 中明确 `RawEvent -> SanitizedEvent -> StructuredContext` 的生产路径，测试禁止原始文本越过 `PrivacyAirGap`。
5. 在 `aios-spec` 中定义 `DecisionRoute`、`DecisionBackendResult`、`CapabilityLevel`，建立后端能力等级与允许动作的映射。
6. 在 `aios-agent` 中实现 `DecisionRouter` 的优先级路由逻辑（熔断 → 隐私预算 → 语义复杂度 → 能力等级），把现有 `MockCloudProxy` 迁移为 `RuleBasedBackend`。
7. 实现 `FallbackNoOp` 兜底后端，确保所有降级路径均返回合法 `IntentBatch`。
8. 在 `PolicyEngine` 中加入 `CapabilityLevel` 校验：拒绝超出后端能力的意图；输出侧区分 `SuggestedAction` 和 `AuthorizedAction`。
9. 接入本地小模型或云端大模型后端，统一返回 `DecisionBackendResult`。
10. 将系统态采集接入同一 Rust collector 入口，不改变 core、agent 和 action 的上层协议。

## 参考 (References)

- [架构概览](../architecture/index.md)
- [设计哲学](../architecture/philosophy.md)
- [Daemon 架构设计](../architecture/pipeline.md)
- [Android 接口最小可运行边界](../android/collector.md)
- [Android Collector README](https://github.com/114August514/DiPECS/blob/main/apps/android-collector/README.md)
