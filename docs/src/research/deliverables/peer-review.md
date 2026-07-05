# Peer Review

DiPECS 是面向 Android 平台的本地优先 AIOS 原型系统，探索智能操作系统中的本地感知、窗口级上下文构造、决策路由、授权执行与可测量资源优化闭环。系统采集应用切换、通知、设备状态等 Android 信号，经 Privacy Air-Gap 脱敏并聚合为结构化上下文，再由本地规则或可选 LLM 生成意图；动作必须经过本地策略与生命周期审查，形成 AuthorizedAction 后才可执行。项目已实现 Android 采集器、Rust daemon、Replay/Audit 与安全 Action Bridge，并通过 PreWarm、PrefetchFile 和升级后的 ReleaseMemory 实验证明部分 Android-safe 动作能带来启动延迟、预取等待和内存压力指标改善。隐私与审计是可信执行的边界，不替代性能和表现收益本身。
