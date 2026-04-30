# 架构概览

DiPECS 采用 **机制-策略分离**（Mechanism-Policy Separation）的架构原则，将系统分为三个平面：

## 分层架构

| 层级 | 模块 | 职责 |
| :--- | :--- | :--- |
| 应用层 | Kotlin App | 权限申请、行为采集、UI 交互、优化动作调用 |
| 核心层 | Rust Core (`aios-core`) | 事件聚合、脱敏引擎、策略校验、Trace 回放 |
| 云端层 | LLM + Skills | 场景理解、Skill 编排、置信度判断 |

## 控制平面与数据平面

| 平面 | 职责 | 模块 |
| :--- | :--- | :--- |
| **Control Plane** | 决定"做什么、能不能做" | Intent Parsing、Planning、Policy、Scheduling、Confirmation |
| **Data Plane** | 负责"执行与数据流动" | IPC/RPC、事件采集、数据脱敏、动作执行、Trace 记录 |

## 数据流

```
采集 (Kotlin) → 序列化 → 聚合脱敏 (Rust) → 云端 LLM → Skills 判断
                                                      ↓
优化执行 (Kotlin) ← 策略校验 (Rust) ← 结构化输出 ←───┘
     ↓
Trace 记录 → Golden Trace 回归验证
```

## 相关文档

- [状态机设计](states.md)
- [RFC 提案](rfc/process.md)
