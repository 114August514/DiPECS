# 设计哲学

> Status: Partially current  
> Last verified: 2026-06-30  
> 本页描述架构原则。当前可运行实现以 [当前实现总览](../current/overview.md)、[动作治理](../current/action-governance.md) 和源码为准。

## OS = 对象 + API

DiPECS 重新定义了操作系统的基本对象：

| 传统 OS | DiPECS |
|:--------|:-------|
| `Process`, `File`, `Socket` | `Intent` (意图), `Action` (动作), `Context` (上下文), `Policy` (策略) |

API 就是这些对象之间的转换函数：

```text
solve(Intent, Context)   → Plan<Actions>
verify(Action, Policy)   → AuthorizedAction | Denied
```

## 五大核心模块

### 1. `aios-spec` — 宪法层

整个项目的 **Single Source of Truth**。定义核心数据结构、Trait 接口和 Function Calling Schema。

工程意义：只要 `spec` 不动，各组可以并行开发。协议变更必须走 RFC 流程。云端返回的 JSON 必须完美契合这里的 Rust `struct`——模型升级导致格式变化时，spec 必须能兼容。

### 2. `aios-action` — 触手层

AIOS 与底层 Android/Linux 的动作执行边界。它只接收 `PolicyEngine` 审查通过的 `AuthorizedAction`，不读取 app、collector 或 agent 的内部状态。

- **PreWarmProcess**：预热目标应用进程
- **PrefetchFile**：预取热点文件到页缓存
- **KeepAlive**：保活当前或目标进程
- **ReleaseMemory**：释放非关键内存
- **NoOp**：安全兜底

当前实现保留本地 replay fallback，并已经把 Android 可执行的 `PrefetchFile(url:/uri:)` 通过 authenticated localhost bridge 接到 Android collector。更高权限的 syscall 路线不作为当前 Android public-API 主线。

### 3. `aios-core` — 脊梁层

这是系统的 **Action Bus（动作总线）**，核心职责：

- **调度**：决定哪个 Action 先执行
- **策略引擎 (Policy Engine)**：内核级"防火墙"，根据 Policy 判定 AI 产生的动作是否安全（例如深夜不能自动支付、转账必须人工确认）
- **Privacy Filter (隐私滤镜)**：数据出海前进行正则或轻量语义脱敏——这是 DiPECS 最核心的模块之一
- **Action Verifier**：云端 LLM 可能产生错误指令，执行前必须做 100% 静态类型检查和安全过滤
- **Trace Engine**：全链路确定性记录，支持 Golden Trace 回归验证

实现方式：Rust 同步优先（不引入不必要的 async），异步点集中在系统边界。

### 4. `aios-agent` — 决策层

agent 接收 `StructuredContext`, 负责选择最小足够的推理后端，并统一返回 `IntentBatch`：

- **DecisionRouter**：根据脱敏后的 `StructuredContext` 选择 rule-based、本地小模型、云端 LLM 或 fallback
- **Capability 声明**：每个后端声明最大风险等级和允许动作类型
- **降级策略**：云端超时或不可用时，使用本地保守策略或 `FallbackNoOp`

### 5. `aios-collector` — 采集层

Rust 侧采集层入口，负责对接 app 侧采集能力和后续下沉到 daemon / system 的来源，并统一产出 `CollectorEnvelope` / `RawEvent`：

- **App source**：接收 `apps/android-collector` 通过 JSONL / JNI / local socket 传入的原始观测
- **System source**：接收 `/proc`、Binder probe、系统状态采集等 daemon/system 来源
- **Schema boundary**：校验 schema 版本、来源等级和传输批次边界

## 分层决策的数据流

大脑可以在云端，但安全边界必须在本地。DiPECS 的本质是一个带隐私隔离、能力分级和授权审查的语义执行器。

```text
apps/android-collector / daemon sources
    -> aios-collector
    -> CollectorEnvelope / RawEvent
    -> PrivacyAirGap
    -> WindowAggregator
    -> DecisionRouter
    -> PolicyEngine
    -> AuthorizedAction
    -> ActionExecutor
    -> Trace
```

主链路环节：

1. **Collection** — Android app 或 system source 产生原始观测
2. **Ingress** — `aios-collector` 规范化为 `CollectorEnvelope` / `RawEvent`
3. **Redaction** — `PrivacyAirGap` 抹除 PII，输出 `SanitizedEvent`
4. **Aggregation** — `WindowAggregator` 生成 `StructuredContext`
5. **Reasoning** — `DecisionRouter` 选择规则、本地、云端或 fallback 后端
6. **Authorization** — `PolicyEngine` 结合 `CapabilityLevel` 审查动作
7. **Execution** — `ActionExecutor` 只执行 `AuthorizedAction`
8. **Observation** — `ActionResult` 和 Trace 进入回归验证

系统要解决的最核心问题不是"模型准不准"，而是**语义鸿沟**：云端说"把这个文件发给张三"，本地 OS 必须精准定位——哪个文件？哪个张三？对应的 fd 是什么？App 权限够不够？

> Action Bus 是 AI 时代的系统调用接口。传统 `syscall` 传的是寄存器数值，AI-syscall 传的是语义对象。
>
> — JYY

## 一个意图的生命周期

以"给张三发 50 块红包"走一遍完整流程：

1. **输入**：Experience Layer 捕获语音，产生原始 `Intent`
2. **解析** (`aios-agent`)：模型通过 Memory 找到张三的 ID，制定计划 `[Search(张三), Pay(50)]`
3. **分发** (`aios-core`)：动作进入 Action Bus
4. **审计** (Policy Engine)：查询策略，发现"支付额度 > 20 需要人工确认"
5. **交互**：弹出确认框给用户
6. **执行** (`aios-action`)：用户确认后，通过 action executor 调用支付
7. **观测**：管理员看到支付 Action 生命周期结束，状态变为 `COMPLETED`

## 工程防线

- **`data/traces`** — 离线轨迹数据是算法组的"粮草"。开发时大量依赖离线 Trace 回放测试 Action Bus 逻辑，而非每次都调云端 API
- **`tools/aios-replay`** — 调试组的"时光机"。系统崩溃时一帧帧重放失败过程，定位错误 Action
- **`docs/rfc`** — 架构组的"刹车闸"。防止接口每天变化导致项目无法编译
- **`scripts/setup-env.sh`** — 新人的"入职礼"。10 分钟内跑通 Hello World
