# 架构概览

DiPECS 采用 **机制-策略分离**（Mechanism-Policy Separation）的架构原则。系统分为三个物理层和两个逻辑平面。

## 分层架构

| 层级 | 模块 | 语言 | 职责 |
| :--- | :--- | :--- | :--- |
| 应用层 | `apps/android-collector` | Kotlin | Android 公开 API 采集能力验证、权限申请、trace preview |
| 采集层 | `aios-collector` | Rust | 对接 app 侧采集能力和后续 system 来源，统一产出 `CollectorEnvelope` / `RawEvent` |
| 守护进程层 | Rust Daemon (`dipecsd`) | Rust | 长驻运行、采集 task、处理管道装配 |
| 核心层 | Rust Crates | Rust | 隐私脱敏、窗口聚合、决策路由、策略校验、授权动作执行 |
| 云端层 | LLM + Skills | — | 高复杂度场景理解、Skill 编排、置信度判断 |

依赖方向：`aios-spec → collector/core/action/agent → aios-daemon`

上层负责业务逻辑和外部通信，下层负责数据类型定义和策略执行。跨层通信通过 `aios-spec` 中定义的结构体和 Trait 完成，不允许反向依赖。

## 控制平面与数据平面

| 平面 | 回答的问题 | 包含模块 |
| :--- | :--- | :--- |
| **Control Plane** | 做什么、能不能做 | Intent Parsing、Planning、PolicyEngine、Scheduling、Confirmation |
| **Data Plane** | 如何执行、数据如何流动 | IPC/Binder、事件采集、数据脱敏、动作执行、Trace 记录 |

两个平面的关键约束：**Control Plane 决策必须先于 Data Plane 执行，Data Plane 不得绕过 Control Plane 直接动作。**

## 数据流

```text
apps/android-collector / daemon sources
    -> aios-collector (ingress + normalize)
    -> CollectorEnvelope / RawEvent
    -> aios-core (PrivacyAirGap -> WindowAggregator -> StructuredContext)
    -> aios-agent (DecisionRouter -> rule-based / local / cloud / fallback)
    -> IntentBatch
    -> aios-core (PolicyEngine + CapabilityLevel)
    -> aios-action (AuthorizedAction only)
    -> ActionResult / Trace
```

主链路的设计原则见[设计哲学](philosophy.md)。

## 阅读指南

根据你的角色和目标选择入口：

| 我想... | 阅读顺序 |
| :--- | :--- |
| **快速了解系统** | `overview.md` → `philosophy.md` → `crates-map.md` |
| **理解为什么这样设计** | `philosophy.md` → `../research/background/aios-arch.md` |
| **开始写 daemon 代码** | `crates-map.md` → `daemon-architecture.md` → `states.md` |
| **写 Android 端采集代码** | `../research/background/android-data-sources.md` → `android-interface-mvp.md` |
| **改 Android 动作层** | `android-action-boundary.md` → `philosophy.md` → `../research/deliverables/feasibility.md` |
| **提交设计变更** | `rfc/process.md` → `docs/templates/rfc/0000-template.md` |
| **了解项目背景** | `../research/deliverables/requirements.md` → `../research/deliverables/feasibility.md` |
| **新成员入职** | `index.md` → `overview.md` → `crates-map.md` → `../team/dev.md` |

## 相关文档

- [设计哲学](philosophy.md) — 五大模块深度拆解与意图生命周期
- [代码地图](crates-map.md) — 代码仓库的文件级导览
- [Daemon 架构设计](daemon-architecture.md) — 最精确的技术规格
- [状态机设计](states.md) — 核心状态转移逻辑
- [AIOS 参考架构](../research/background/aios-arch.md) — 理论基石
- [RFC 提案](rfc/process.md) — 变更提案流程
