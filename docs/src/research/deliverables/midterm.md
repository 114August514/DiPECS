# 中期报告：最小可执行原型

> 答辩 slides：[下载 PPTX](../../slides/midterm/DiPECS-midterm-v3.pptx)

## 阶段目标

完成"用户行为采集 → 本地脱敏整理 → 云端 Skills 判断 → 本地优化执行 → 结果记录"的最小闭环。

## 已完成事项

### 基础采集

- [x] UsageStatsManager 应用使用采集
- [x] NotificationListenerService 通知事件采集
- [x] 基础上下文（时间、网络、电量）采集

### 核心链路

- [x] Rust 事件模型 (`aios-spec`) — `RawEvent`、`SanitizedEvent`、`StructuredContext` 等类型体系
- [x] 隐私脱敏引擎 (`PrivacyAirGap`) — RawEvent → SanitizedEvent，原始 PII 不可恢复
- [x] 窗口聚合器 (`WindowAggregator`) — 10s 时间窗口，自动构建 `ContextSummary`
- [x] 云端通信骨架 (`MockCloudProxy`) — 6 种信号 → 意图生成规则

### 策略与执行

- [x] 策略引擎 (`PolicyEngine`) — 风险等级 + 置信度双重校验
- [x] 动作执行器 (`DefaultActionExecutor`) — 5 种动作类型骨架
- [x] 动作总线 (`ActionBus`) — mpsc channel 解耦采集与处理
- [x] 完整处理管道 — Collection → Sanitize → Aggregate → Infer → Evaluate → Execute

### 测试与验证

- [x] 63 个测试，全部通过
- [x] 覆盖：脱敏 (5) / 窗口聚合 (17) / 策略引擎 (11) / 动作执行 (14) / 云端模拟 (9) / 动作总线 (7)
- [x] GoldenTrace 数据结构已定义，骨架就绪

## 架构变更

中期阶段完成了一次关键重构：daemon 二进制已独立为 `aios-daemon`，修正了反向依赖，恢复 `spec` 作为协议中心、`core` 负责审查、`collector` 负责采集、`agent` 负责决策、`action` 负责授权动作执行的边界。详见 [v0.2 发布说明](../../design/releases/v0.2.md)。

## 待解决问题

- Cloud LLM HTTPS 通信：已由 `CloudLlmBackend` 接入 DeepSeek/Qwen/OpenAI-compatible endpoint。
- Android → Rust 生产入口：已通过 append-only JSONL 与 `dipecsd --android-trace-jsonl` 接入；JNI 可作为后续替换路线。
- GoldenTrace / runtime trace：replay、audit hash 与 `RuntimeTraceRecorder --trace-output` 已接入。
- 真机 / 模拟器部署验证

## 中期评审结论

中期基线之后，项目已经从最小可执行原型推进到可回放、可审计、可接 Android public-API 数据源的生产 ingress 原型。仍需补充真机验证、更多 Android-safe 动作和最终实验数据。
