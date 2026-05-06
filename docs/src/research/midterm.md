# 中期报告：最小可执行原型

> 答辩 slides：[slides.md](../../slides/midterm/slides.md)

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

中期阶段完成了一次关键重构：daemon 二进制从 `aios-adapter` 迁移至 `aios-agent`，修正了反向依赖，恢复 `spec → core → kernel → adapter → agent` 的正确层级。详见 [v0.2 发布说明](../design/releases/v0.2.md)。

## 待解决问题

- MockCloudProxy → 真实 HTTPS 通信（reqwest + rustls）
- Kotlin → Rust JNI bridge（NotificationListenerService 事件传入 daemon）
- GoldenTrace 录制与回放接入主循环
- 真机 / 模拟器部署验证

## 中期评审结论

<!-- TODO: 填入评审意见 -->
