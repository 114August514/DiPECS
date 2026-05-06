# DiPECS

DiPECS（Digital Intelligence Platform for Efficient Computing Systems）是一个运行在 Android 设备上的意图驱动系统原型，旨在构建一条 **本地采集 → 脱敏整理 → 云端大模型调用 Skills 判断 → 本地执行优化 → Skills 自迭代** 的闭环。

## 核心目标

1. **Privacy First**：本地优先处理，零泄漏边界，原始数据不出设备。
2. **Intent Driven**：云端大模型根据结构化上下文预测用户意图，本地执行低风险优化。
3. **Deterministic & Observable**：全链路 Trace 记录，支持状态回放与确定性回归验证。
4. **Extensible Skills**：Skills 根据用户反馈和历史行为持续自迭代。

## 技术路线

| 层级 | 语言 | 职责 |
| :--- | :--- | :--- |
| Android 应用层 | Kotlin | 权限、服务、UI、行为/上下文采集、优化动作执行 |
| 核心逻辑层 | Rust | 事件模型、上下文聚合、脱敏、策略校验、Trace 回放 |
| 云端判断层 | Remote LLM + Skills | 场景理解、Skill 选择与调用、置信度判断、Skill 自迭代 |

## 文档导航

完整目录见左侧边栏。按目标速查：

| 我想... | 从哪开始 |
| :--- | :--- |
| 理解项目背景与可行性 | [需求分析](research/requirements.md) → [可行性报告](research/feasibility.md) |
| 了解系统架构 | [架构概览](design/overview.md) → [设计哲学](design/philosophy.md) |
| 开始写代码 | [代码地图](design/crates-map.md) → [开发指南](team/dev.md) |
| 提交设计变更 | [RFC 提案](design/rfc/process.md) |
