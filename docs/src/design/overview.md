# 架构概览

DiPECS 采用 **机制-策略分离**（Mechanism-Policy Separation）的架构原则。系统分为三个物理层和两个逻辑平面。

## 分层架构

| 层级 | 模块 | 语言 | 职责 |
| :--- | :--- | :--- | :--- |
| 应用层 | Kotlin App | Kotlin | 权限申请、行为采集、UI 交互、优化动作调用 |
| 核心层 | Rust Core (`dipecsd`) | Rust | 事件聚合、脱敏引擎、策略校验、Trace 回放 |
| 云端层 | LLM + Skills | — | 场景理解、Skill 编排、置信度判断 |

依赖方向：`aios-agent → aios-adapter → aios-kernel → aios-core → aios-spec`

上层负责业务逻辑和外部通信，下层负责数据类型定义和策略执行。跨层通信通过 `aios-spec` 中定义的结构体和 Trait 完成，不允许反向依赖。

## 控制平面与数据平面

| 平面 | 回答的问题 | 包含模块 |
| :--- | :--- | :--- |
| **Control Plane** | 做什么、能不能做 | Intent Parsing、Planning、PolicyEngine、Scheduling、Confirmation |
| **Data Plane** | 如何执行、数据如何流动 | IPC/Binder、事件采集、数据脱敏、动作执行、Trace 记录 |

两个平面的关键约束：**Control Plane 决策必须先于 Data Plane 执行，Data Plane 不得绕过 Control Plane 直接动作。**

## 数据流

```text
采集 (Kotlin) → 序列化 → 聚合脱敏 (Rust) → 云端 LLM → Skills 判断
                                                     ↓
优化执行 (Kotlin) ← 策略校验 (Rust) ← 结构化输出 ←──┘
     ↓
Trace 记录 → Golden Trace 回归验证
```

六个环节的详细说明见[设计哲学](philosophy.md)。

## 阅读指南

根据你的角色和目标选择入口：

| 我想... | 阅读顺序 |
| :--- | :--- |
| **快速了解系统** | `overview.md` → `philosophy.md` → `crates-map.md` |
| **理解为什么这样设计** | `philosophy.md` → `../research/aios-arch.md` |
| **开始写 daemon 代码** | `crates-map.md` → `daemon-architecture.md` → `states.md` |
| **写 Android 端采集代码** | `../research/android-data-sources.md` → `android-interface-mvp.md` |
| **提交设计变更** | `rfc/process.md` → `rfc/0000-template.md` |
| **了解项目背景** | `../research/requirements.md` → `../research/feasibility.md` |
| **新成员入职** | `index.md` → `overview.md` → `crates-map.md` → `../team/dev.md` |

## 相关文档

- [设计哲学](philosophy.md) — 五大模块深度拆解与意图生命周期
- [代码地图](crates-map.md) — 代码仓库的文件级导览
- [Daemon 架构设计](daemon-architecture.md) — 最精确的技术规格
- [状态机设计](states.md) — 核心状态转移逻辑
- [AIOS 参考架构](../research/aios-arch.md) — 理论基石
- [RFC 提案](rfc/process.md) — 变更提案流程
