# Changelog

## v0.2 — 2026-05-05

端到端处理管道打通：采集 → 脱敏 → 聚合 → 模拟推理 → 校验 → 执行。

### Added

- `aios-agent`: MockCloudProxy 模拟 LLM 决策，6 种信号→意图规则
- `aios-kernel`: DefaultActionExecutor 骨架，5 种动作类型
- `aios-core`: WindowAggregator 10s 时间窗口聚合
- `aios-core`: PolicyEngine 策略校验（风险/置信度/动作过滤）
- `aios-core`: ActionBus 事件与意图通道
- 63 个测试（MockCloudProxy 9, ActionExecutor 14, WindowAggregator 17, PolicyEngine 11, ActionBus 7, PrivacyAirGap 5）

### Changed

- daemon 主循环从单线程重构为 2-task tokio 管道（采集 + 处理）
- 依赖层级修正：`aios-adapter` 不再反向依赖 `aios-agent`

### Fixed

- `ExtensionCategory` 补充 Hash derive
- `ActionResult` 从 aios-spec 正确导出

## v0.1 — 2026-04

项目初始化。aios-spec 宪法层 + aios-core 核心逻辑 + adapter 采集骨架。

### Added

- `aios-spec`: 事件类型、上下文、意图、轨迹、公共 trait
- `aios-core`: PrivacyAirGap 脱敏引擎
- `aios-adapter`: BinderProbe / ProcReader 采集骨架
- CI 基础设施（lint, test, build, audit）
