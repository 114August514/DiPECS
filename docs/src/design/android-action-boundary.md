# Android 动作能力边界与 ActionExecutor 当前实现

> 日期: 2026-06-06  
> 范围: 基于 DiPECS 当前文档约束，整理 Android 公开 API 条件下可落地的“真实动作”，并给出 `aios-action` 的改造方向。

进一步的具体实施步骤见 [Android 动作实现手册](android-action-implementation.md)。

## 目标

这份文档回答两个问题:

1. 在 **仅使用 Android 官方公开 API、API >= 33** 的前提下，DiPECS 到底能执行哪些本地优化动作？
2. 结合当前仓库的 `ActionType` / `ActionExecutor` 设计，哪些地方应该改，哪些地方不应该继续沿着 Linux syscall 方向做？

相关项目约束与现状:

- 项目要求“仅使用 Android 官方公开 API（API >= 33）”。见 [需求分析](../research/deliverables/requirements.md)。
- 本地端只执行低风险、可审计、可关闭的优化动作。见 [可行性分析](../research/deliverables/feasibility.md)。
- 当前 `aios-spec` 中动作类型包括 `PreWarmProcess`、`PrefetchFile`、`KeepAlive`、`ReleaseMemory` 和 `NoOp`。见 [`crates/aios-spec/src/intent.rs`](../../../crates/aios-spec/src/intent.rs)。
- 当前 `aios-action` 保留本地 replay fallback，同时已经能把 `PrefetchFile(url:/uri:)` 转发到 Android localhost bridge。Android 侧通过 `auth_token` 鉴权、payload 大小限制、读超时和失败退避保护 action socket。见 [`crates/aios-action/src/lib.rs`](../../../crates/aios-action/src/lib.rs) 和 [`apps/android-collector`](../../../apps/android-collector/README.md)。

## 当前已落地状态

- Rust 侧 `PolicyEngine` 只输出经过授权的 `AuthorizedAction`。
- Rust 侧 `aios-action` 默认保留 replay fallback；当启用 `DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=true` 且提供 `DIPECS_ANDROID_ACTION_BRIDGE_TOKEN` 时，会把 Android 可执行的 `PrefetchFile(url:/uri:)` 转发到 Android collector。
- Android 侧 `AuthorizedActionSocketServer` 只监听 `127.0.0.1`，但每个 payload 必须包含 `auth_token`。
- token 由 Android collector 生成，存储在 `EncryptedSharedPreferences`；UI 只脱敏显示，可通过 Copy Action Socket Token 主动复制。
- socket 读取限制为 64KB，并设置读超时；空 payload、无效 JSON、超大 payload、鉴权失败都会进入失败退避。
- Android 侧当前可执行动作集中在 `PrefetchFile`，目标限制为 `url:http(s)://...` 或 app 有权访问的 `uri:content://...`。

## 结论先行

如果坚持当前项目约束，那么:

- **可以做**: 预取自己可访问的数据、调度自己的后台任务、拉起自己的前台服务、清理自己的缓存和资源、通过通知引导用户进入目标应用。
- **不能做**: 静默预热第三方应用进程、修改第三方进程 `oom_score_adj`、直接释放第三方应用内存、绕过 Android 后台启动限制去强拉别的应用 UI。

换句话说，DiPECS 的动作层更适合被定义为:

- **Rust 侧**: 审核动作、选择动作、记录 trace、生成平台无关的授权结果。
- **Kotlin / Android 侧**: 调用公开 Android API 执行低风险优化。

而不应该继续把主线目标放在 `/proc/pid/reclaim`、`process_madvise()`、`fork zygote` 这类系统态接口上。

## Android 公开 API 下的动作边界

### 1. `PreWarmProcess`

当前设想:

- 预热目标应用进程
- 提前 fork zygote
- 在用户真正打开前先把目标 App “热起来”

结论:

- **不能**对第三方 App 按这个语义实现。
- Android 公开 API 没有给普通应用“静默预热别的应用进程”的稳定能力。
- 背景启动 Activity 从 Android 10 起受到严格限制，不能把“预热”退化成后台强拉 UI。

公开 API 下可保留的能力:

- 预热**你自己的** isolated service / app process。
- 预热**你自己的**依赖、缓存、模型、索引或网络连接。
- 通过通知或 `PendingIntent` 在用户交互后进入目标应用，而不是后台直接启动。

更合适的语义:

- `WarmOwnService`
- `WarmOwnResources`
- `PostLaunchHint`

### 2. `PrefetchFile`

当前设想:

- 预取热点文件到页缓存
- 在用户下一步操作前，把关键文件/资源提前准备好

结论:

- **可以做**，但目标必须是 **当前应用有权限访问的内容**。
- 不能静默访问第三方应用私有目录。

公开 API 下可落地的对象:

- 当前应用自己的 internal storage / app-specific external storage
- 用户通过 SAF 授权给应用的 `Uri` / 目录
- `MediaStore` 中应用有读取权限的共享媒体
- 网络侧的预取任务，例如提前拉取元数据、缩略图、索引页、聊天会话摘要

更合适的语义:

- `PrefetchAccessibleContent`
- `PrefetchUri`
- `PrefetchNetworkResource`

### 3. `KeepAlive`

当前设想:

- 调整 `oom_score_adj`
- 提高目标进程存活性
- 在低风险场景下保持当前或目标进程常驻

结论:

- **不能**按“修改第三方或任意进程保活参数”的方式实现。
- 公开 API 不允许普通应用修改别的进程 `oom_score_adj`。
- 前台服务也不是“任意保活开关”，它要求任务对用户可感知，并且伴随常驻通知；Android 12+ 还额外限制了后台启动前台服务。

公开 API 下可保留的能力:

- 保持 **DiPECS 自己** 的执行链在合理条件下继续运行
- 使用 `WorkManager` / expedited work 调度短任务
- 使用 `JobScheduler` 调度可延迟或带约束任务
- 在用户可感知的长任务场景下使用前台服务
- 特定设备配对场景下再考虑 Companion Device Manager 豁免

更合适的语义:

- `KeepOwnPipelineAlive`
- `ScheduleExpeditedWork`
- `StartUserVisibleForegroundService`

### 4. `ReleaseMemory`

当前设想:

- 通过 `/proc/pid/reclaim`
- 通过 `process_madvise(MADV_COLD)`
- 释放目标进程的非关键内存

结论:

- **不能**按“回收任意第三方 App 内存”的方式实现。
- Android 的全局回收策略属于系统内存管理与 `lmkd` 范畴，不是普通应用可以对外控制的接口。

公开 API 下可保留的能力:

- 清理 DiPECS 自己的缓存
- 停止不必要的 worker / service / coroutine
- 丢弃本地预取结果、释放自己的图片缓存和索引缓存
- 在 Android 14+ 上，`killBackgroundProcesses()` 也只允许第三方应用杀死**自己的**进程

更合适的语义:

- `TrimOwnCaches`
- `ReleaseOwnResources`
- `CancelPrefetchWork`

### 5. `NoOp`

这个动作没有边界问题，建议保留。

它仍然是:

- 熔断后的安全降级
- 云端结果不可执行时的保守返回
- Golden Trace 中最稳定的基线动作

## 当前动作模型与公开 API 适配表

| 当前动作 | 当前注释方向 | 公开 API 可行性 | 建议处理 |
| :--- | :--- | :--- | :--- |
| `PreWarmProcess` | fork zygote / 预热目标 App | 不可直接实现 | 改语义，收缩为“预热自身组件或发布启动提示” |
| `PrefetchFile` | 预取热点文件到页缓存 | 可部分实现 | 保留动作类型，但把目标限制为“当前应用可访问内容” |
| `KeepAlive` | 调整 `oom_score_adj` | 不可直接实现 | 改为调度自身工作链、前台服务或 WorkManager |
| `ReleaseMemory` | `/proc/pid/reclaim` / `process_madvise` | 不可直接实现 | 改为释放自身缓存、取消任务、关闭自身资源 |
| `NoOp` | 安全降级 | 可实现 | 保留 |


## 参考资料

- Android Developers: [Background activity launch restrictions](https://developer.android.com/guide/components/activities/secure-bal)
- Android Developers: [Foreground services overview](https://developer.android.com/develop/background-work/services/fgs)
- Android Developers: [Restrictions on starting a foreground service from the background](https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start)
- Android Developers: [Task scheduling / WorkManager](https://developer.android.com/develop/background-work/background-tasks/persistent)
- Android Developers: [`JobInfo.Builder.setPrefetch(boolean)`](https://developer.android.com/reference/android/app/job/JobInfo.Builder#setPrefetch(boolean))
- Android Developers: [Access app-specific files](https://developer.android.com/training/data-storage/app-specific)
- Android Developers: [Storage Access Framework](https://developer.android.com/guide/topics/providers/document-provider)
- Android Developers: [`ZygotePreload`](https://developer.android.com/reference/android/app/ZygotePreload)
- Android Developers: [`ActivityManager.killBackgroundProcesses`](https://developer.android.com/reference/android/app/ActivityManager#killBackgroundProcesses(java.lang.String))
- AOSP: [Low memory killer daemon](https://source.android.com/docs/core/perf/lmkd)
