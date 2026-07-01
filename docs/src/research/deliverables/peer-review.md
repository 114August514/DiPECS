# Peer Review

DiPECS 是面向 Android/Linux 的本地优先 AIOS 原型系统，探索隐私友好的感知、决策与执行闭环。系统采集应用切换、通知、设备状态等本地信号，经 Privacy Air-Gap 脱敏并聚合为结构化上下文，再由规则引擎或可选云端大模型生成意图。所有动作需经本地 PolicyEngine 审查，形成 AuthorizedAction 后才可执行。项目已实现 Android 采集器、Rust daemon、审计回放与安全 action bridge，为可控、可审计的端侧智能系统提供实践参考。
