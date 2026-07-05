# Changelog

## v0.3 — 2026-07-05

真机动作收益与预测基线闭环版本：从“动作链路可执行”推进到“部分 Android-safe 动作具备可测正收益”。

### Added

- `PreWarmProcess own:*`: 接入 Android-safe 自有资源预热路径，并用 Pixel 6a n=20/mode hit/miss 测量接入 LSApp standard split 净收益 gate。
- `PrefetchFile`: 支持 `url:` / `uri:` 目标经 Android action bridge 预取到 app-owned cache，并新增真机 hit/miss 成本采集与 same-budget net-benefit gate。
- `ReleaseMemory cache:volatile`: 新增 app-owned volatile memory cache 释放语义，可由 `PreWarmProcess own:volatile-cache:<MB>` seed，并在真内存压力场景下释放。
- next-app action-value 评估接入真实 LSApp standard hit@1、Pixel 6a 设备成本和 `StrongPredictiveActionBaseline` 同预算对照。
- 真机采集脚本新增 n≥20、bridge `status=ok`、artifact existence、pressure observed、Welch significance 等 fail-closed gate。

### Changed

- ReleaseMemory 的收益语义从删除磁盘 prefetch cache 收敛为释放 app-owned volatile memory；旧 `cache:prefetch` 不再作为内存压力收益使用。
- PreWarm 净收益 fixture 刷新为 Pixel 6a 实测输入：DiPECS `net_benefit_ms=76,068,875.158`，高于 strong baseline `72,283,770.198`。
- PrefetchFile 净收益按真机测得的 hit saved、miss fetch+read cost 和 dispatch/control cost 计算；DiPECS projected net benefit 高于 strong baseline。
- Android action bridge 的收益实验统一按授权动作 execute envelope、HMAC、freshness window 和设备端终态回执判定。
- KeepAlive 保持动作链路覆盖，但普通 app 形态下不再进入正面收益列表；抗杀收益留给 platform-signed / system-image 部署验证。

### Fixed

- 修正真机系统时钟偏差导致 action execute envelope 被 freshness check 拒绝的问题：采集侧支持按设备时钟偏移生成新鲜信封。
- PrefetchFile 采集在读取缓存前等待文件大小稳定，避免把未完成下载当成命中读延迟。
- ReleaseMemory 压力采集区分“未观察到压力”和“观察到压力但效果不显著”，避免把缺失压力证据误判为动作收益。

## v0.2 — 2026-05-05

端到端处理管道打通：采集 → 脱敏 → 聚合 → 模拟推理 → 校验 → 执行。

### Added

- `aios-agent`: MockCloudProxy 模拟 LLM 决策，6 种信号→意图规则
- `aios-action`: DefaultActionExecutor 骨架，5 种动作类型
- `aios-core`: WindowAggregator 10s 时间窗口聚合
- `aios-core`: PolicyEngine 策略校验（风险/置信度/动作过滤）
- `aios-core`: ActionBus 事件与意图通道
- 63 个测试（MockCloudProxy 9, ActionExecutor 14, WindowAggregator 17, PolicyEngine 11, ActionBus 7, PrivacyAirGap 5）

### Changed

- daemon 主循环从单线程重构为 2-task tokio 管道（采集 + 处理）
- 依赖层级修正：`aios-collector` 不再反向依赖 `aios-agent`

### Fixed

- `ExtensionCategory` 补充 Hash derive
- `ActionResult` 从 aios-spec 正确导出

## v0.1 — 2026-04

项目初始化。aios-spec 宪法层 + aios-core 核心逻辑 + adapter 采集骨架。

### Added

- `aios-spec`: 事件类型、上下文、意图、轨迹、公共 trait
- `aios-core`: PrivacyAirGap 脱敏引擎
- `aios-collector`: BinderProbe / ProcReader 采集骨架
- CI 基础设施（lint, test, build, audit）
