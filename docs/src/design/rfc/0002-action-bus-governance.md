# RFC-0002: Action Bus 治理边界与生命周期审计

## 摘要 (Summary)

把现有「`SuggestedAction` → `AuthorizedAction` → executor」的隐式管线，收敛为一个显式的**动作治理状态机**：模型只能提出不可信的 `ActionProposal`，经本地 schema / 隐私 / 策略校验后才生成可执行的 `AuthorizedAction`，每一步状态迁移都落一条 `AuditRecord`，且任何 proposal 都必有且仅有一个**终态审计**。落地范围对应 issue #4（类型治理边界）、#5（生命周期 + 终态审计覆盖）、#8（OfflineAdapter 执行闭环），务实 P0，不引入 Capability/Budget/Scheduler 的完整实现。

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
- **务实裁剪**。Capability/Budget 只保留**枚举占位 + 扩展点**（issue 验收明确允许），不实现租约/调度评分。

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

- **Capability / Budget / Scheduler**：只留枚举占位与扩展点。
- 理由：现有动作集仅 5 种、全是本地低风险动作，**撑不起**租约/调度评分这类机制。在信息最少时设计它们，第一版几乎必错；等真实负载（高风险动作、真机 adapter）出现再做，代价是局部插桩，而非重构。

> 一句话标尺：**拔不出来的抽象现在做对，拔得出来的抽象等需求来了再做。** 后续 PR 评审遇到「要不要现在就抽象 X」时，用这把尺子量。

## 设计方案 (Design)

### 1. 类型治理边界（issue #4，`aios-spec`）

核心原则：**不可信输入与可执行动作是两个不能隐式互转的类型**。

```rust
// 不可信侧 —— planner / LLM / agent 的产物。
// 复用现有 SuggestedAction 作为「裸提议」，包一层带治理元数据的 Proposal。
pub struct ActionProposal {
    pub intent_id: String,
    pub action: SuggestedAction,   // 现有类型，不动
    pub effect: EffectClass,       // 由 action_type 推导，见下
    pub proposed_at_ms: i64,
}

// 可执行侧 —— 唯一的构造路径是 PolicyEngine 审查通过。
// 字段保持私有，外部无法手搓；adapter 只认这个类型。
pub struct AuthorizedAction {
    pub intent_id: String,
    pub action: SuggestedAction,
    pub effect: EffectClass,
    pub authorized_at_ms: i64,
}
```

- `AuthorizedAction` **不再实现** `From<ActionProposal>`/`Deserialize` 的公开构造捷径；唯一来源是 core 的状态机（下节）。这是「不能隐式互转」的类型级保证。
- `EffectClass`（按现有 5 个动作的真实副作用分级，不照搬文档 6 级）：

  | ActionType | EffectClass |
  | --- | --- |
  | `NoOp` | `PureRead` |
  | `PrefetchFile` / `KeepAlive` | `LocalCacheWrite` |
  | `PreWarmProcess` / `ReleaseMemory` | `LocalStateChange` |

- schema 校验拒绝：缺失必需 target（`PreWarmProcess`）、未知 action type（serde 天然拒绝）、risk/effect 非法组合（如 `PureRead` 配 `High`）。
- `RiskLevel` 维持现有三级（`Low/Medium/High`），**不加** `Critical`——现有动作集没有特权动作，加了是死代码。

### 2. 生命周期状态机与终态审计（issue #5，`aios-spec` + `aios-core`）

#### 2.1 状态枚举（精简版，~12 态）

按 DiPECS 真实管线裁剪文档的 18 态——去掉未实现的 `BudgetReserved`/`Scheduled`/`Retrying`/`RolledBack`/`Expired`/`Cancelled`：

```rust
pub enum ActionState {
    // 正常路径
    Proposed,
    SchemaValidated,
    RedactionChecked,
    PolicyChecked,
    Dispatched,
    Succeeded,        // 终态
    // 拒绝/失败终态
    RejectedInvalidSchema,    // 终态
    RejectedPrivacyViolation, // 终态
    DeniedByPolicy,           // 终态
    Failed,                   // 终态
}
```

#### 2.2 状态机：`ActionLifecycle`（core 新模块 `action_lifecycle.rs`）

这是真正的「Action Bus 内核」，**包住**现有 `PolicyEngine`：

```text
ActionProposal
  → Proposed
  → SchemaValidated      (validate: target/effect/risk 组合)   ─┬─ 失败 → RejectedInvalidSchema (终态)
  → RedactionChecked     (target 必须是脱敏实体, 非 raw 包名)   ─┼─ 失败 → RejectedPrivacyViolation (终态)
  → PolicyChecked        (复用 PolicyEngine.evaluate_*)         ─┼─ 拒绝 → DeniedByPolicy (终态)
  → Dispatched           (交给 adapter)
  → Succeeded (终态)                                            └─ adapter Err → Failed (终态)
```

#### 2.3 审计记录与强制规则

```rust
pub struct AuditRecord {
    pub intent_id: String,
    pub action_type: ActionType,
    pub target: Option<String>,
    pub effect: EffectClass,
    pub transitions: Vec<ActionState>,  // 完整迁移序列
    pub terminal: ActionState,          // 终态 (冗余但便于查询/golden)
    pub denial_reason: Option<DenialReason>,  // 复用现有枚举
    pub error: Option<String>,
}
```

强制不变量（用单测 + 终态覆盖测试钉死）：

1. 每个 `ActionProposal` 产出**恰好一条** `AuditRecord`，且 `terminal` 必为终态之一。
2. 成功路径记录完整迁移序列 `[Proposed, …, Succeeded]`。
3. 四类失败各有对应终态，互不混淆。
4. 全程**不 panic**：所有错误走结构化 `LifecycleError` / `DenialReason` / `AdapterError`。

### 3. OfflineAdapter 执行闭环（issue #8，`aios-action`）

新增一个纯离线、确定性的 adapter，与现有 `DefaultActionExecutor`（含 Android bridge）并存。它只认 `AuthorizedAction`，是 replay/CI 的执行层。

```rust
pub trait ActionAdapter {
    fn name(&self) -> &'static str;
    fn execute(&self, authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError>;
}

pub struct OfflineAdapter { /* Arc<Mutex<SimulatedState>> */ }
```

- 支持动作（映射到现有 5 类 + 模拟语义）：`NoOp`、`PreWarmProcess`→SimulatePrewarm、`PrefetchFile`→SimulateCache、`KeepAlive`、`ReleaseMemory`。
- **不**访问真实系统 / 网络 / Android；输出 deterministic `ActionOutcome`（不含 wall-clock、不含随机 latency——latency 由上层注入或固定 0）。
- unsupported 动作返回结构化 `AdapterError::Unsupported`，**绝不 panic**，也不破坏状态机。
- adapter 失败 → 状态机落 `Failed` 终态，窗口继续处理后续动作。

### 4. 集成点

- **daemon `pipeline.rs`**：`process_window` 在 `PolicyEngine` 之后，把每条 approved action 走一遍 `ActionLifecycle`，收集 `AuditRecord` 写入现有 runtime trace JSON（新增 `"audit"` 字段）。
- **cli `replay`**：同样收集 `AuditRecord`，在 summary 里新增 `Audit records` 计数；`audit_hash` 的输入扩展为包含终态序列（确定性来源：状态机纯函数 + OfflineAdapter）。
- 向后兼容：现有 `ActionExecutor` trait 与 `DefaultActionExecutor` **保留不动**，OfflineAdapter 是新增并行实现。

### 5. 已定决策：审计轨迹纳入 `audit_hash`

> **决策**：`audit_hash` 的输入**扩展为包含每个动作的终态序列**，而非让审计流旁路 hash。

权衡：

- **纳入（采纳）**：审计轨迹成为确定性证明的一部分——「同输入 → 同迁移序列 → 同终态」被 golden 测试钉死。代价是一次性刷新 #6 的 golden hash 基线。
- **旁路（否决）**：golden 测试零改动，但回放无法证明审计轨迹的确定性，留下「审计可能不稳定却测不出来」的盲区。

理由：审计轨迹本就应当是确定性的一部分——一个动作每次回放走过的状态、落到的终态若会漂移，审计就失去了可信度。把它锁进 hash 才是这套治理层的意义所在。基线刷新是可控的一次性成本（同 PR 完成 + PR 描述说明 hash 输入扩展）。

## 影响面 (Impact)

- **涉及的模块**：`aios-spec`（新增 `ActionProposal`/`EffectClass`/`ActionState`/`AuditRecord`/`AdapterError`，调整 `AuthorizedAction`）、`aios-core`（新增 `action_lifecycle.rs`）、`aios-action`（新增 `OfflineAdapter`）、`aios-daemon`/`aios-cli`（接审计流）。
- **接口变更**：`AuthorizedAction` 增加 `effect` 字段并收紧构造路径——会触及现有构造点（policy_engine、测试、Android bridge payload）。
- **向后兼容性**：现有 golden trace 的脱敏/策略/执行三层语义不变；`audit_hash` 输入扩展属于**有意的确定性升级**，需同步刷新 golden 基线（一次性）。

## 风险与缓解 (Risks)

- **风险：收紧 `AuthorizedAction` 构造破坏 Android bridge 序列化**。缓解：bridge payload 仍 `Serialize` `AuthorizedAction`，只是禁止外部 `Deserialize` 反向构造可执行动作；用编译期 + 单测双重保证。
- **已知成本：audit_hash 基线变更影响 #6 golden 测试**（已采纳「纳入」方案，见设计第 5 节）。处理：在同一 PR 内刷新基线，并在 PR 描述说明 hash 输入的扩展。
- **风险：范围蔓延到 Capability/Budget**。缓解：本 RFC 明确只留枚举占位与扩展点，不实现租约/调度。

## 替代方案 (Alternatives)

### 方案 A：全量实现设计文档的 Action Bus（Capability + Budget + Scheduler + 18 态）

体量大、需多分支多周，且会重写现有精简管线和 golden 测试。当前动作集（5 类）撑不起这套机制，多数会是死代码。**不采用**。

### 方案 B：只加 AuditRecord，不做类型分离

最省事，但留下「planner 输出直接进 adapter」的类型漏洞——正是 #4 要堵的核心安全边界。**不采用**。

### 方案 C：把状态机放进 aios-action 而非 aios-core

违反 `spec → core → adapter` 单向依赖：治理决策（policy/redaction）属于 core，adapter 只负责执行。**不采用**。

## 迁移计划 (Migration Plan)

1. `aios-spec`：加类型 + 调整 `AuthorizedAction`，跑通 serde roundtrip 与非法 action 测试。
2. `aios-core`：`action_lifecycle.rs` 状态机 + 终态覆盖测试。
3. `aios-action`：`OfflineAdapter` + 每动作单测 + unsupported 测试。
4. `aios-daemon`/`aios-cli`：接审计流，刷新 golden 基线。
5. 全量 `cargo test --workspace` + `cargo clippy -- -D warnings` 通过。

## 参考 (References)

- `docs/src/refs/papers/Action_Bus_设计参考.docx` — Action Bus 完整设计（本 RFC 的务实裁剪来源）。
- `docs/src/refs/papers/DiPECS参考建议.docx` — P0 最小闭环建议。
- Issues #4 / #5 / #8 — 本 RFC 的验收标准来源。
- [RFC-0001](0001-layered-collection-and-decision-routing.md) — 分层采集与决策路由（本 RFC 在其管线上叠加治理层）。
