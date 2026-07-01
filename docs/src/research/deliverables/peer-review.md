# Peer Review

DiPECS 是面向 Android/Linux 的本地优先 AIOS 原型系统，探索智能操作系统中的感知、决策、授权执行与审计闭环。系统采集应用切换、通知、设备状态等本地信号，经 Privacy Air-Gap 脱敏并聚合为结构化上下文，再由本地规则或可选 LLM 生成意图。动作必须经过本地策略与生命周期审查，形成 AuthorizedAction 后才可执行。项目已实现 Android 采集器、Rust daemon、Replay/Audit 与安全 Action Bridge，为 AIOS 提供机制策略分离、可回放可审计的实践方案。
