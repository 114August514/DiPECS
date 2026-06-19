# RFC-0002: Action Bus 治理边界与生命周期审计

## 摘要 (Summary)

把现有「`SuggestedAction` → `AuthorizedAction` → executor」的隐式管线，收敛为一个显式的**动作治理状态机**：模型的不可信 `SuggestedAction` 在边界处被 core 包装为治理 envelope `ActionProposal`，经本地 schema / 策略校验后，由 `aios-core` 状态机**唯一构造**出可执行的 `AuthorizedAction`（私有字段、不可跨 crate 伪造），每一步状态迁移都落一条带确定性坐标 `ActionCoord` 的 `AuditRecord`，且每个动作必有且仅有一个**终态审计**。落地范围对应 issue #4（类型治理边界）、#5（生命周期 + 终态审计覆盖）、#8（OfflineAdapter 执行闭环）。务实 P0：Capability 拒绝是真实终态；Budget/调度机制、privacy 终态当前无可实现支撑，故不引入对应 variant（已在「范围与 issue 验收对齐」声明）。

## 动机 (Motivation)

当前实现已能跑通最小闭环，但存在三个结构性缺口（对照 `Action_Bus_设计参考` 与 `DiPECS参考建议`）：

1. **治理边界靠约定而非类型**。`AuthorizedAction` 只是 `SuggestedAction` 的包装，二者可隐式互转；没有类型层面阻止「planner 输出直接进 adapter」的路径（#4）。
2. **没有可回放的生命周期**。窗口处理只产出聚合的 trace JSON，单个 action 从 proposed 到终态的状态迁移不可见，也没有「每个动作必有唯一终态审计」的保证（#5）。
3. **执行层是直连 stub**。`DefaultActionExecutor` 直接 `tracing::info!`，没有一个纯离线、确定性、可单测、能被 replay 锁定的 adapter 抽象（#8）。

这三点正是设计文档所谓「syscall governance layer」的最小内核。补上之后，`aios-cli replay` 的每一条动作都会有完整、确定的审计轨迹——项目从「愿景」变「可证明的 runtime 原型」。

## 设计评估 (Evaluation)

- **不推倒现有管线**。`PrivacyAirGap` / `DecisionRouter` / `PolicyEngine` / 窗口聚合都保留，新状态机**包住** `PolicyEngine`，而不是替换它。
- **不破坏 golden trace**。现有 `ReplayResult` 三层校验（脱敏/策略/执行）语义不变；新增的 `AuditRecord` 流是**叠加**的可观测层，golden hash 的输入按需扩展而非重定义。
- **贴合真实的 5 个 action 类型**。不强行套用文档里 11 种 `ActionType` 的宏大枚举；`EffectClass` 按现有动作的真实副作用分级。
- **务实裁剪，但区分「裁实现」与「裁验收」**。Capability 检查**已是真实路径**（`PolicyEngine` 已有 `RiskExceedsCapability`/`ActionCapabilityDenied`），故 `DeniedByCapability` 是真实可达终态。Budget 在现有代码中无任何机制，故 `ActionState` **不引入** `DeniedByBudget` variant（不留不可达死代码）——这一裁剪会改动 issue #5 的验收勾选，已在「范围与 issue 验收对齐」一节显式声明并同步 issue，不悄悄缩范围。

## 抽象边界取舍原则 (Abstraction Boundary Principle)

本 RFC 不追求「一次设计到位」，也不是「能跑就行」。两者都是伪命题。真正的判据只有一条：

> **某个抽象如果做错，纠正的代价有多大？**

据此把抽象分两类，区别对待：

### 现在就钉死（错了会「拔不出来」）

渗透进每一个调用点、一旦长歪事后极难回收的抽象。本 RFC 只有两个：

1. **类型治理边界**（`ActionProposal` vs `AuthorizedAction`）。它是项目的核心论点「模型只提议、本地才授权」。一旦放任「建议直接进 executor」的代码路径出现，后续每个调用方都会复制这条捷径，再回头收紧就是全仓手术。
2. **生命周期 + 终态审计骨架**（`ActionState` / `AuditRecord`）。「每个动作恰好一条终态审计」这条不变量必须从第一行代码就成立，否则审计是补不全的——漏掉的迁移无法事后重建。

### 现在留口、不实现（错了只是「局部返工」）

挂在状态机中间的叶子能力，未来插入不影响既有调用方：

- **Budget / Scheduler**：本 RFC 不引入对应 `ActionState` variant，只在设计上留扩展点（注意：**Capability 不在此列**——它在 `PolicyEngine` 中已是真实实现路径，对应 `DeniedByCapability` 真实终态，详见 §2.1）。
- 理由：现有动作集仅 5 种、全是本地低风险动作，**撑不起**租约/调度评分这类机制。在信息最少时设计它们，第一版几乎必错；等真实负载（高风险动作、真机 adapter）出现再做，代价是局部插桩，而非重构。

> 一句话标尺：**拔不出来的抽象现在做对，拔得出来的抽象等需求来了再做。** 后续 PR 评审遇到「要不要现在就抽象 X」时，用这把尺子量。

## 设计方案 (Design)

### 1. 类型治理边界（issue #4，`aios-spec` + `aios-core`）

核心原则：**不可信输入与可执行动作是两个不能隐式互转的类型**，且「不可伪造」必须由编译器跨 crate 强制，而非靠约定。

#### 1.1 类型归属（解决「私有字段又要跨 crate 构造」的矛盾）

reviewer 指出的硬伤：若 `AuthorizedAction` 留在 `aios-spec` 且字段私有，`aios-core` 反而无法构造它；若字段 `pub`，任何 crate 都能 struct-literal 手搓，不可伪造性失效。结论是**构造器必须和类型同处一个 crate**，所以按「谁是唯一生产者，类型就放谁那」重新归属：

- `ActionProposal`（不可信侧）+ 所有协议数据类型（`EffectClass`/`ActionState`/`AuditRecord`/`ActionOutcome`/`AdapterError`）→ 留在 `aios-spec`，是协议单一真相。
- `AuthorizedAction`（可执行侧）→ **从 `aios-spec` 移到 `aios-core::governance`**，字段私有，唯一构造器 `pub(crate)`，只能被同 crate 的 `ActionLifecycle` 在策略通过后调用。
- `ActionAdapter` trait 跟随 `AuthorizedAction` 也定义在能看到该类型的层；`aios-action` **新增对 `aios-core` 的依赖**（`core` 不依赖 `action`，无环），其 adapter 只能接收 core 递来的 `AuthorizedAction`，无法自行构造。

> 这是对「`aios-spec` 是协议唯一真相」的**有意偏离**：不可伪造性是一条*逻辑不变量*，必须和产生它的逻辑（状态机）放在一起才能由编译器保证。spec 仍持有全部线缆类型（proposal/audit/outcome）；移走的只有那个「凭证」类型本身。替代方案（spec 内 sealed-trait + 见证 token）能把类型留在 spec，但构造器仍得在 core，徒增样板，见替代方案 D。

```rust
// aios-spec —— 不可信侧治理 envelope。core 在边界处为不可信的 SuggestedAction
// 建立（填入确定性 coord + 推导 effect）；裹的 action 字段本身不可信。全 pub、可反序列化。
pub struct ActionProposal {
    pub intent_id: String,         // runtime 关联用（含随机 UUID），不进 canonical hash
    pub coord: ActionCoord,        // 确定性坐标，进 canonical hash（见 §2.4）
    pub action: SuggestedAction,   // 现有类型，不动
    pub effect: EffectClass,       // 由 core 可信侧推导填入（见 §1.2），非外部输入
    pub proposed_at_ms: i64,
}

// 确定性动作坐标：不含 UUID/wall-clock，纯位置量，单次 run 内唯一且 replay 间稳定。
pub struct ActionCoord {
    pub window_ordinal: u32,       // 窗口在 replay 序列中的确定性序号（跨窗口去碰撞）
    pub intent_ordinal: u32,       // intent 在 batch.intents 中的下标
    pub action_ordinal: u32,       // action 在 intent.suggested_actions 中的下标
}

// aios-core::governance —— 可执行侧。字段私有，唯一构造路径是状态机。
pub struct AuthorizedAction {
    intent_id: String,             // runtime 关联（volatile）
    coord: ActionCoord,            // 确定性坐标
    action: SuggestedAction,
    effect: EffectClass,
    authorized_at_ms: i64,
}

impl AuthorizedAction {
    // pub(crate)：仅 ActionLifecycle 在 PolicyChecked 后可调用，外部 crate 无法手搓。
    pub(crate) fn seal(proposal: &ActionProposal, authorized_at_ms: i64) -> Self { /* ... */ }
    // 只读 getter 对外暴露，adapter 据此执行；但无法反向构造。
    pub fn action(&self) -> &SuggestedAction { &self.action }
    pub fn effect(&self) -> EffectClass { self.effect }
    pub fn coord(&self) -> ActionCoord { self.coord }
}
```

- `AuthorizedAction` **不实现** `Deserialize`——否则任何 crate 反序列化即可绕过状态机伪造一个可执行动作。Android bridge 仍可 `Serialize` 它发往手机（只读方向安全），但不能 `Deserialize` 反向构造，见风险节。
- adapter 侧只拿到 `&AuthorizedAction`、只能调 getter；`aios-action` 无 `pub` 构造器可用 → 「planner 输出直接进 adapter」在类型层不可表达。

#### 1.2 effect 由可信侧推导

`effect` 不是 proposal 的「输入字段」而是 core 在建 proposal 时按 `action_type` 计算的派生量（reviewer 第 2 点）。`EffectClass`（按现有 5 个动作的真实副作用分级，不照搬文档 6 级）：

  | ActionType | EffectClass |
  | --- | --- |
  | `NoOp` | `PureRead` |
  | `PrefetchFile` | `LocalCacheWrite` |
  | `PreWarmProcess` / `KeepAlive` / `ReleaseMemory` | `LocalStateChange` |

- `KeepAlive` 归 `LocalStateChange`：它改变进程存活与资源占用，副作用强于单纯缓存写（采纳 reviewer 建议）。
- schema 校验拒绝：缺失必需 target（`PreWarmProcess`）、未知 action type（serde 天然拒绝）、risk/effect 非法组合（如 `PureRead` 配 `High`）。
- `RiskLevel` 维持现有三级（`Low/Medium/High`），**不加** `Critical`——现有动作集没有特权动作，加了是死代码。

### 2. 生命周期状态机与终态审计（issue #5，`aios-spec` + `aios-core`）

#### 2.1 状态枚举（精简版，9 态）

按 DiPECS 真实管线裁剪文档的 18 态。**只保留当前可达的状态**，不引入任何不可达的死 variant（budget/调度类终态延后到机制存在时的后续 RFC；privacy 终态延后到 typed target 存在时，见下）：

```rust
pub enum ActionState {
    // 正常路径
    Proposed,
    SchemaValidated,
    PolicyChecked,
    Dispatched,
    Succeeded,        // 终态
    // 拒绝/失败终态（均当前可达）
    RejectedInvalidSchema,    // 终态
    DeniedByCapability,       // 终态：PolicyEngine 的 RiskExceedsCapability/ActionCapabilityDenied
    DeniedByPolicy,           // 终态：风险超配置 / 置信度过低 / target 不在上下文
    Failed,                   // 终态：adapter Err
}
```

裁剪与保留的依据（回应 reviewer 第 5 点，逐项对齐 issue #5 状态建议）：

- **保留 `DeniedByCapability`**：现有 `PolicyEngine` 已有 `RiskExceedsCapability` / `ActionCapabilityDenied`，是**真实可达**的拒绝路径，必须有独立终态，不能并进 `DeniedByPolicy`。
- **去掉 `RedactionChecked` / `RejectedPrivacyViolation`**（reviewer 第 4 轮第 3 点）：当前**没有可实现的 privacy predicate**。`SuggestedAction.target` 是裸 `Option<String>`，不携带来源/脱敏证明；`PrivacyAirGap` 原样保留 package name（`privacy_airgap.rs:144-158,189-204`）；唯一相关检查是 target 是否在 `KnownTargets` 中，失败已归 `TargetNotInContext`/`DeniedByPolicy`（`policy_engine.rs:262-293`）。无法区分「未脱敏 target」与「合法但不在上下文的字符串」，故该终态当前不可测。**移出本 RFC，留在 issue #5**，待引入 typed `ResourceTarget` + provenance/opaque sanitized ID 后的后续 RFC 再做。
- **去掉 `Running`**：OfflineAdapter 的 `execute` 是同步纯函数，`Dispatched → Succeeded/Failed` 之间无可观测的运行态。
- **`DeniedByBudget` / `BudgetReserved` / `Scheduled`**：现有代码**无任何 budget/调度机制**，故本 RFC 的 `ActionState` 枚举**不引入**这些 variant（避免不可达的死 variant）——预算/调度终态留给「机制存在时的后续 RFC」。这一项使 issue #5「预算拒绝有对应终态」**暂不勾选**，已在「范围与 issue 验收对齐」声明。
- **去掉 `Retrying`/`RolledBack`/`Expired`/`Cancelled`**：无重试/回滚/超时/取消语义，纯死代码。

#### 2.2 状态机：`ActionLifecycle`（core 新模块 `action_lifecycle.rs`）

这是唯一的 Action Bus 内核，**内部持有并调用** `PolicyEngine`——不是「PolicyEngine 之后再过一遍」（消除 reviewer 第 3 点的双管线/重复执行）。输入是现有 PolicyEngine 真实需要的整批上下文，而非裸 proposal；`window_ordinal` 由调用方**显式传入**（reviewer 第 4 轮第 2 点），不靠 `&self` 内部计数器：

```rust
impl ActionLifecycle {
    // 唯一入口。batch/capability/ctx 与现有 PolicyEngine.evaluate_batch_with_context
    // 对齐；window_ordinal 由调用方（replay/daemon 驱动循环）显式传入，
    // 状态机保持纯函数、无隐藏可变状态。
    pub fn run(
        &self,                       // 内部持有 &PolicyEngine 与 &dyn ActionAdapter（不可变）
        window_ordinal: u32,         // 驱动循环按窗口处理顺序赋号，组装 ActionCoord
        batch: &IntentBatch,
        capability: &CapabilityLevel,
        ctx: &StructuredContext,
    ) -> Vec<AuditRecord> { /* 每个 (intent, ordinal, suggested_action) 产出恰好一条 */ }
}
```

- **`coord` 的唯一性范围**：`ActionCoord` 只在**单次 replay/run（即一段连续的窗口序列）内全局唯一**，是 *canonical replay 坐标*，不是跨进程的运行时全局主键。`window_ordinal` 由该次 run 的驱动循环从 0 单调赋号——同一条 trace 重放永远得到同一串坐标（确定性）。
- **不把 canonical coord 当运行时全局 ID**：daemon 的 append-only runtime trace 跨进程重启会从 0 重置 `window_ordinal`。若后续需要「跨重启唯一」的运行时标识，另加 **volatile `session_id` / `runtime_action_id`**（含启动随机量），与 `intent_id` 同列入 `VOLATILE_KEYS` 剥离、**不进 hash**——绝不污染 canonical replay 坐标。

每条 `(intent, ordinal, suggested_action)` 在边界处被 core 炸开为一个 `ActionProposal`（生成确定性坐标、推导 `effect`），然后单独走状态机：

```text
SuggestedAction (来自 Intent.suggested_actions[ordinal])
  → core 建 ActionProposal (坐标 = (window_ordinal, intent_ordinal, action_ordinal), 推导 effect)
  → Proposed
  → SchemaValidated      (validate: target/effect/risk 组合)   ─┬─ 失败 → RejectedInvalidSchema (终态)
  → PolicyChecked        (消费 PolicyActionDecision)            ─┼─ 能力不足 → DeniedByCapability (终态)
                                                                ─┼─ 其他拒绝 → DeniedByPolicy (终态)
  → AuthorizedAction::seal(...)   ← 唯一构造点（只有 lifecycle，不是 PolicyEngine）
  → Dispatched           (交给唯一的 dispatch 抽象 ActionAdapter)
  → Succeeded (终态)                                            └─ adapter Err → Failed (终态)
```

> privacy 终态（`RedactionChecked`/`RejectedPrivacyViolation`）已移出本 RFC，理由见 §2.1——当前无可实现的 privacy predicate，留待 typed `ResourceTarget` 引入后的后续 RFC。

- **唯一 dispatch 抽象**：`ActionLifecycle` 持有一个 `&dyn ActionAdapter`。`DefaultActionExecutor`（含 Android bridge）和 `OfflineAdapter` 都实现这同一个 trait，运行时二选一注入。一个 `AuthorizedAction` 只会被 dispatch 一次，不存在「旧 executor 路径 + 新状态机」并行重复执行——旧的 `ActionExecutor::execute` 直连路径在 daemon/cli 中被状态机取代。

#### 2.2.1 PolicyEngine 契约改造：从「构造授权」到「逐 action 裁决」

reviewer 第 2 轮第 2 点：现状 `PolicyEngine.evaluate_*` 返回 `Vec<AuthorizedAction>`（即 PolicyEngine 自己在构造授权凭证），与「lifecycle 唯一 seal」直接冲突；且 `PolicyDecision.action_denials: Vec<DenialReason>` **无 ordinal**，混合通过/拒绝时拒绝原因无法映射回具体 action，「每 proposal 一条正确终态」不成立。

改造（落到 `aios-core::policy_engine`）：

```rust
// PolicyEngine 不再产出 AuthorizedAction，只产出逐 action 的裁决。
pub struct PolicyActionDecision {
    pub intent_ordinal: u32,
    pub action_ordinal: u32,
    pub verdict: PolicyVerdict,        // Approved | Denied(DenialReason)
}
// intent 级拒绝（risk/confidence/capability 不满足）展开为该 intent
// 每个 action_ordinal 各一条 Denied，verdict 携带对应 DenialReason。
```

- `PolicyEngine` 的职责收敛为「裁决」：遍历 `intent.suggested_actions` 时**带 enumerate 下标**，每条产出一个 `PolicyActionDecision`，approve/deny 都带 `(intent_ordinal, action_ordinal)`。`PolicyActionDecision` 只承载 batch 内的二元下标；`window_ordinal` 不在 PolicyEngine 视野内，由 `ActionLifecycle` 在窗口边界用驱动循环的窗口序号补齐，组装成完整三元 `ActionCoord`（见 §2.4）。
- `AuthorizedAction` 的构造从 `policy_engine.rs` **移除**；唯一 `seal` 点是 `ActionLifecycle` 在消费到 `verdict == Approved` 的 proposal 时。这样「谁授权」与「谁裁决」分离：PolicyEngine 裁决、lifecycle 授权。
- `action_denials: Vec<DenialReason>`（无 ordinal）被 `Vec<PolicyActionDecision>` 取代；现有 `PolicyDecision` 的调用方（daemon/cli/测试）随之改。risk/confidence 仍读自 `Intent`，不进 proposal。

#### 2.3 审计记录与强制规则

```rust
pub struct AuditRecord {
    pub coord: ActionCoord,             // 确定性主键 (window_ordinal, intent_ordinal, action_ordinal)，进 hash，见 §2.4
    pub intent_id: String,              // 运行时关联（volatile：含 UUID，canonical 投影剥离，不进 hash）
    pub action_type: ActionType,
    pub target: Option<String>,
    pub effect: EffectClass,
    pub transitions: Vec<ActionState>,  // 完整迁移序列
    pub terminal: ActionState,          // 终态 (冗余但便于查询/golden)
    pub outcome: Option<ActionOutcomeSummary>, // 成功时写入 adapter outcome 的确定性摘要，进 hash（见下）
    pub denial_reason: Option<DenialReason>,  // 复用现有枚举
    pub error: Option<String>,
}
```

- **`outcome` 字段**（reviewer 第 6 轮第 1 点）：`ActionAdapter::execute` 返回 `ActionOutcome`，但若审计只记 `(coord, terminal, transitions)`，两次都 `Succeeded` 但模拟结果不同时审计与 hash 完全看不出来，违反 issue #8 的「执行结果可写入 audit log」「replay outcome 稳定」。故成功路径把 adapter outcome 投影为**确定性** `ActionOutcomeSummary`（不含 wall-clock/随机）写入 `outcome`，并**纳入 `audit_hash`**；失败路径 `outcome=None`、信息走 `error`。

强制不变量（用单测 + 终态覆盖测试钉死）：

1. 每个 `ActionProposal`（即每个 `coord`）产出**恰好一条** `AuditRecord`，且 `terminal` 必为终态之一。
2. 成功路径记录完整迁移序列 `[Proposed, …, Succeeded]`，且 `outcome` 为 `Some(确定性摘要)`。
3. 四类终态（schema / capability / policy / failed）各有对应记录，互不混淆（privacy 终态延后，见 §2.1）。
4. 全程**不 panic**：所有错误走结构化 `LifecycleError` / `DenialReason` / `AdapterError`。
5. **outcome 确定性**：重复 replay 同一 trace，`outcome` 逐字段相等；outcome 内容漂移必然改变 `audit_hash`（新增回归测试钉死）。

#### 2.4 稳定动作标识：确定性 `ActionCoord` vs 运行时 `intent_id`

reviewer 第 2 轮第 1 点：一条 Intent 可含多条甚至重复的 suggested action，需稳定标识证明「每个 proposal 恰好一条终态审计」；但**不能把随机 `intent_id`（`Uuid::new_v4()`）嵌进主键再纳入 hash**——现有 canonical audit 正是靠按 key 剥离 `intent_id` 才保证三次回放 hash 一致，UUID 一旦进 hash 即回归。reviewer 第 3 轮第 1 点：`(intent_ordinal, action_ordinal)` 只在单个 batch 内唯一，replay 跨多个窗口时第二个窗口的 `(0,0)` 会与第一个碰撞，主键失效。

方案：**运行时标识与 canonical 标识分离**，且坐标含窗口序号保证全局唯一。

- **canonical 主键 `coord: ActionCoord = (window_ordinal, intent_ordinal, action_ordinal)`**：窗口在 replay 序列中的确定性序号 + intent 在 `batch.intents` 中的下标 + action 在 `intent.suggested_actions` 中的下标。`window_ordinal` 由 replay 驱动循环按窗口处理顺序单调赋号（确定性、无 wall-clock、无 UUID），跨窗口全局唯一，重复 action 也能区分。它是 `AuditRecord` 的主键，**纳入 `audit_hash`**。
- **运行时关联 `intent_id: String`**：仍保留在 `AuditRecord` 里供日志/调试关联真实 intent，但它含 UUID，属 volatile——`canonicalize` 把 `intent_id` 加入 `VOLATILE_KEYS` 一并剥离，**不进 hash**（与现有 `event_id`/`window_id` 同等处理）。
- `coord` 贯穿 `ActionProposal` → `AuthorizedAction` → `ActionOutcome` → `AuditRecord`。
- 「恰好一条终态审计」的测试按 `coord` 分组断言：每个 coord 出现且仅出现一次、且为终态。**测试场景必须含至少两个窗口、且两窗口出现重复 `(intent_ordinal, action_ordinal)`**，证明 `window_ordinal` 消除碰撞、全局主键成立。
- **新增确定性回归**：扩展现有 `audit_hash_is_stable_across_repeated_runs`，断言「三次相同 replay 的 action audit hash 完全一致」，钉死 UUID 不泄漏进 hash。

### 3. OfflineAdapter 执行闭环（issue #8，`aios-action`）

新增一个纯离线、确定性的 adapter。它与现有 `DefaultActionExecutor`（含 Android bridge）**实现同一个 `ActionAdapter` dispatch trait**，运行时二选一注入状态机——这就是 §2.2 说的「唯一 dispatch 抽象」，两条执行路径收敛于此，不并行。

```rust
// 定义在能看到私有 AuthorizedAction 的层（随 AuthorizedAction，见 §1.1）。
pub trait ActionAdapter {
    fn name(&self) -> &'static str;
    fn execute(&self, authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError>;
}

pub struct OfflineAdapter { /* Arc<Mutex<SimulatedState>> */ }
// DefaultActionExecutor 改为 impl 同一个 trait，复用现有 Android bridge 转发逻辑。
```

- 支持动作（映射到现有 5 类 + 模拟语义）：`NoOp`、`PreWarmProcess`→SimulatePrewarm、`PrefetchFile`→SimulateCache、`KeepAlive`、`ReleaseMemory`。`ActionType` 是封闭 5-variant 枚举，OfflineAdapter **完整覆盖全部 variant**。
- **不**访问真实系统 / 网络 / Android；输出 deterministic `ActionOutcome`（不含 wall-clock、不含随机 latency——latency 由上层注入或固定 0）。
- **非法/未知动作归 schema 阶段，不设 `AdapterError::Unsupported`**（reviewer 第 6 轮第 2 点）：`ActionType` 封闭且 adapter 全覆盖，安全 Rust 中「adapter 收到不认识的 variant」**不可达**，留 `Unsupported` 是测不到的死路径。未知 `action_type`（如反序列化到不存在的字符串）在更早的 **schema/反序列化阶段**就失败，归 `RejectedInvalidSchema`（已有可达终态）。`AdapterError` 仍保留，但只表达**真实执行失败**（模拟资源不可用等）→ 状态机落 `Failed`。
- adapter 失败 → 状态机落 `Failed` 终态，窗口继续处理后续动作。

### 范围与 issue 验收对齐（回应 reviewer 第 5 点）

本 RFC 有意调整了 issue 的部分验收范围，在此显式声明哪些会被关闭、哪些延后，不悄悄缩范围：

- **issue #4**：全部验收项本 RFC 覆盖（类型分离、adapter 只收 `AuthorizedAction`、schema 拒绝、serde roundtrip、非法 action 测试）。其中「`ResourceBudget`、带副作用 action 的 budget」**降级为字段占位不校验**——需在 issue #4 补注说明。**可关闭**（带范围注记）。
- **issue #5**：覆盖 schema/capability/policy/failed 四类终态 + 「恰好一条终态审计」+ 不 panic。**不覆盖**：（a）「隐私违规有对应终态」——当前无可实现的 privacy predicate（`target` 是裸 `String`，无来源/脱敏证明），`RejectedPrivacyViolation` 延后到 typed `ResourceTarget` 引入后的后续 RFC；（b）「预算拒绝有对应终态」（`ActionState` 不引入 `DeniedByBudget`）与 `Scheduled`/`BudgetReserved`/`Expired`/`Cancelled`（均无机制支撑）。→ 在 issue #5 勾掉已完成项，对未做项注明承接的后续 RFC，**该 issue 不由本 PR 完全关闭**。
- **issue #8**：验收项覆盖，但**「unsupported action 有明确错误类型」一项改判**（reviewer 第 6 轮第 2 点）：`ActionType` 封闭、adapter 全覆盖，adapter 级 `Unsupported` 不可达。该验收**重释为「非法/未知 `action_type` 在 schema/反序列化阶段被拒，归 `RejectedInvalidSchema`」**——语义等价（非法动作不会被执行），且可测。需在 issue #8 补注此重释。「执行结果可写入 audit log / replay outcome 稳定」由新增 `AuditRecord.outcome` + 纳入 hash 满足。**可关闭**（带此重释注记）。

> 准则：裁实现可以，裁验收必须留痕。任何被本 RFC 推迟的验收项，都会在对应 issue 上注明，并指向承接它的未来工作，避免「RFC 宣称覆盖、issue 却没人关」的悬空。

### 4. 集成点

- **daemon `pipeline.rs`**：`process_window` 不再「先 PolicyEngine 再 executor」两段式。改为构造一个 `ActionLifecycle`（注入 `PolicyEngine` + 选定的 `ActionAdapter`），对每个窗口调用 `lifecycle.run(window_ordinal, batch, capability, ctx)` 一次（`window_ordinal` 由处理循环单调赋号），拿回 `Vec<AuditRecord>` 写入 runtime trace JSON（新增 `"audit"` 字段）。策略检查与 dispatch 都在 lifecycle 内部，只发生一次。跨进程重启 `window_ordinal` 从 0 重置——这是 canonical replay 坐标的预期语义，跨重启唯一性若需要由 volatile `session_id` 承担（§2.2）。
- **cli `replay`**：注入 `OfflineAdapter` 作为 `ActionAdapter`，同样按 replay 窗口序列调 `lifecycle.run(window_ordinal, ...)` 收集 `AuditRecord`；summary 新增 `Audit records` 计数；`audit_hash` 输入扩展为包含按 `coord` 排序的 `(coord, terminal, transitions, outcome)`（确定性来源：state machine 纯函数 + OfflineAdapter 的确定性 outcome；`intent_id` 经 `VOLATILE_KEYS` 剥离不进 hash）。
- **取代而非并存**：旧的「`PolicyEngine.evaluate_*` → `DefaultActionExecutor::execute`」直连调用在 daemon/cli 中被 `ActionLifecycle` 取代。`DefaultActionExecutor` 不删，但改为 `impl ActionAdapter` 后由 lifecycle 统一驱动，避免两条路径重复执行同一动作。
- **收束 seal 旁路（务实信任边界）**（reviewer 第 3–5 轮）：起点是 CLI `SendAuthorizedAction` 和 Android socket/UI 能把任意带 `auth_token` 的 JSON 直接交给 `dispatchAuthorizedActionJson` 执行（`AuthorizedActionSocketServer.kt:145-172`、`MainActivity.kt:365-399`）。**信任模型（P0）**：`auth_token` 认证「调用方可信」；持有本机 socket token 的本地操作者**纳入 P0 信任边界**——不追求跨进程「payload 经 lifecycle seal」的强防伪（HMAC/签名/nonce 对原型是过度防御，已与 reviewer 达成一致）。在此前提下收束而非全封：
  - **常规管线唯一经 lifecycle**：daemon/CLI 的正常动作路径只经 `ActionLifecycle` → `DefaultActionExecutor`（`impl ActionAdapter`，内部把 `AuthorizedAction` 序列化经 socket 转发）。**release 保留 socket 的 `dispatchAuthorizedActionJson` 接收入口**——它正是合法 bridge 的落点，不能 gate 掉（这是上一轮自相矛盾处：gate 掉它会同时切断生产 bridge）。
  - **CLI raw-send 改诊断**：`SendAuthorizedAction` 改为 ping/health-check（独立消息类型，socket 端**不 dispatch 动作**），`android_bridge.rs::load_payload` 的 `manual-prefetch` 构造删除。通用 CLI 不再提供「手搓 AuthorizedAction 发执行端」的口子。
  - **只 gate Android UI 手动按钮**：`MainActivity` 的「Run AuthorizedAction Now / Via Service」属开发期手动触发，gate 到 debug build（debug source set/flavor 移除代码，而非仅运行时 `BuildConfig.DEBUG` 分支）。socket dispatch 入口本身不动。
  - **threat model 表述**：**「常规 daemon/CLI 管线必须经过 lifecycle；持 token 的本地诊断操作者属于显式可信例外」**——不写「release 无任何绕过路径」（在务实信任模型下既不必要也不准确）。迁移计划第 4 步加测试：常规 CLI/daemon 路径无「绕过 lifecycle 构造 AuthorizedAction」的公开 API；UI 手动执行入口在 release source set 不存在。

### 5. 已定决策：审计轨迹纳入 `audit_hash`

> **决策**：`audit_hash` 的输入**扩展为包含每个动作的终态序列**，而非让审计流旁路 hash。

权衡：

- **纳入（采纳）**：审计轨迹成为确定性证明的一部分——「同输入 → 同迁移序列 → 同终态」被 golden 测试钉死。代价是一次性刷新 #6 的 golden hash 基线。
- **旁路（否决）**：golden 测试零改动，但回放无法证明审计轨迹的确定性，留下「审计可能不稳定却测不出来」的盲区。

理由：审计轨迹本就应当是确定性的一部分——一个动作每次回放走过的状态、落到的终态若会漂移，审计就失去了可信度。把它锁进 hash 才是这套治理层的意义所在。基线刷新是可控的一次性成本（同 PR 完成 + PR 描述说明 hash 输入扩展）。

## 影响面 (Impact)

- **涉及的模块**：`aios-spec`（新增 `ActionProposal`/`EffectClass`/`ActionState`/`AuditRecord`（含 `outcome`）/`ActionOutcome`/`ActionOutcomeSummary`/`AdapterError`；**移出** `AuthorizedAction`）、`aios-core`（接收 `AuthorizedAction` + `ActionAdapter` trait，新增 `governance` 与 `action_lifecycle.rs`）、`aios-action`（新增 `OfflineAdapter`，`DefaultActionExecutor` 改 `impl ActionAdapter`，**新增对 `aios-core` 的依赖**）、`aios-daemon`/`aios-cli`（改用 `ActionLifecycle` 单管线，接审计流）。
- **接口变更**：`AuthorizedAction` 移到 core、字段私有、加 `effect`/`coord`、唯一构造器 `pub(crate) seal`——会触及所有现有构造点（policy_engine 返回值、测试、Android bridge payload）。`ActionExecutor` trait（spec）被 `ActionAdapter` 取代为 dispatch 抽象。
- **依赖图变更**：新增 `aios-action → aios-core` 边。已确认 `aios-core` 不依赖 `aios-action`，无环；`spec → core → action` 仍单向。
- **向后兼容性**：现有 golden trace 的脱敏/策略/执行三层语义不变；`audit_hash` 输入扩展属于**有意的确定性升级**，需同步刷新 golden 基线（一次性）。

## 风险与缓解 (Risks)

- **风险：`AuthorizedAction` 移出 spec 触及面大**。缓解：这是 reviewer 第 1 点要求的「可编译的所有权方案」——不可伪造性无法在 spec 内跨 crate 实现。改动集中在构造点，由编译器全量暴露，无静默遗漏。
- **风险：收紧 `AuthorizedAction` 构造破坏 Android bridge 序列化**。缓解：bridge payload 仍 `Serialize`（只读发出方向安全），只禁止 `Deserialize` 反向构造可执行动作；编译期 + 单测双重保证。
- **已知成本：audit_hash 基线变更影响 #6 golden 测试**（已采纳「纳入」方案，见设计第 5 节）。处理：在同一 PR 内刷新基线，并在 PR 描述说明 hash 输入的扩展。
- **风险：范围与 issue 验收不一致**。缓解：见「范围与 issue 验收对齐」一节——被推迟的验收项（budget 终态等）已逐条声明并将在对应 issue 留痕，#5 不由本 PR 完全关闭。

## 替代方案 (Alternatives)

### 方案 A：全量实现设计文档的 Action Bus（Capability + Budget + Scheduler + 18 态）

体量大、需多分支多周，且会重写现有精简管线和 golden 测试。当前动作集（5 类）撑不起这套机制，多数会是死代码。**不采用**。

### 方案 B：只加 AuditRecord，不做类型分离

最省事，但留下「planner 输出直接进 adapter」的类型漏洞——正是 #4 要堵的核心安全边界。**不采用**。

### 方案 C：把状态机放进 aios-action 而非 aios-core

违反 `spec → core → action` 单向依赖：治理决策（policy/redaction）属于 core，adapter 只负责执行。**不采用**。

### 方案 D：`AuthorizedAction` 留在 spec，用 sealed-trait + 见证 token 保不可伪造

可让类型留在 spec：定义一个 `mod sealed` 私有 trait 作为构造见证，仅 core 能产生。但构造器逻辑仍必须在 core，spec 侧只剩一个无法独立构造的空壳，徒增 sealed 样板与一层间接。收益（类型留在 spec）不抵成本，**不采用**，改用方案：直接把类型移到唯一生产者所在的 core（§1.1）。

## 迁移计划 (Migration Plan)

1. `aios-spec`：加 `ActionProposal`/`EffectClass`/`ActionState`/`AuditRecord`（含 `outcome`）/`ActionOutcome`/`ActionOutcomeSummary`/`AdapterError`；**移出** `AuthorizedAction`。跑通 serde roundtrip 与非法 `action_type` 反序列化拒绝测试（归 schema 阶段）。
2. `aios-core`：新建 `governance`（落 `AuthorizedAction` 私有字段 + `seal` + `ActionAdapter` trait）+ `action_lifecycle.rs` 状态机；改 `PolicyEngine` 调用方拿新类型。`PolicyActionDecision` 只带 batch 内 `(intent_ordinal, action_ordinal)`，`window_ordinal` 由 lifecycle 在窗口边界补齐组装成完整 `ActionCoord`。状态机单测 + 终态覆盖 + 「每 `coord` 恰好一条终态审计」测试（含**两个窗口、重复 ordinal** 场景，证明全局主键不碰撞）。
3. `aios-action`：加 `aios-core` 依赖；`OfflineAdapter` + `DefaultActionExecutor` 都 `impl ActionAdapter`；每动作单测 + 成功 `outcome` 摘要确定性测试 + adapter 真实失败→`Failed` 测试（不再有 unsupported-variant 测试，理由见 §3）。
4. `aios-daemon`/`aios-cli`：换成 `ActionLifecycle` 单管线（`run` 显式传 `window_ordinal`），接审计流，刷新 golden 基线；**收束 seal 旁路**——CLI `SendAuthorizedAction` 改为不执行动作的 ping/health-check（见 §4），Android UI 手动执行按钮移到 debug source set；**release 保留 socket `dispatchAuthorizedActionJson` 供 `DefaultActionExecutor` 转发**。加测试：常规 CLI/daemon 路径无「绕过 lifecycle 构造 AuthorizedAction」的公开 API。`replay` 的 `audit_hash` 加「三次回放 hash 一致」+「两窗口重复 ordinal 无碰撞」+「outcome 漂移改变 hash」回归。
5. 全量 `cargo test --workspace` + `cargo clippy -- -D warnings` 通过。
6. 在 issue #4/#5/#8 按「范围与 issue 验收对齐」勾选/留痕；#5 注明延后项。

## 参考 (References)

- `docs/src/refs/papers/Action_Bus_设计参考.pdf` — Action Bus 完整设计（本 RFC 的务实裁剪来源）。
- `docs/src/refs/papers/DiPECS参考建议.pdf` — P0 最小闭环建议。
- Issues #4 / #5 / #8 — 本 RFC 的验收标准来源。
- [RFC-0001](0001-layered-collection-and-decision-routing.md) — 分层采集与决策路由（本 RFC 在其管线上叠加治理层）。
