# Android 动作能力边界与 ActionExecutor 改造建议

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
- 当前 `aios-action` 仍是 tracing stub，但注释里预留的是 `/proc`、`oom_score_adj`、`process_madvise` 等更偏系统态的实现。见 [`crates/aios-action/src/lib.rs`](../../../crates/aios-action/src/lib.rs)。

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

## 推荐改造方案

### 方案 A: 最小改动，保留现有 `ActionType`

如果你希望尽量少动现有协议和测试，这是最稳的做法。

建议这样解释现有动作:

- `PreWarmProcess`: 不再表示“预热第三方 App 进程”，改为“预热 DiPECS 自身组件、缓存或用户可见的启动引导”
- `PrefetchFile`: 改为“预取当前应用有权访问的文件、URI 或网络资源”
- `KeepAlive`: 改为“保持 DiPECS 自身后台执行链继续运行”
- `ReleaseMemory`: 改为“释放 DiPECS 自身缓存与预取资源”

优点:

- `aios-spec` 变动最小
- `PolicyEngine` 与现有测试大多还能保留
- 适合先把“真动作”跑起来

缺点:

- 动作名字会比真实 Android 语义更宽
- 后续读代码的人需要依赖文档理解“收缩后的含义”

### 方案 B: 调整协议，改成更贴近 Android 的动作模型

如果你希望长期维护更清晰，建议把 `ActionType` 改成更像 Android 公开 API 的形状，例如:

- `WarmOwnService`
- `PrefetchAccessibleContent`
- `ScheduleExpeditedWork`
- `StartUserVisibleForegroundService`
- `TrimOwnCaches`
- `PostPredictionNotification`
- `NoOp`

优点:

- 语义和平台能力一致
- 文档、策略、实现、演示更容易讲清楚
- 后续 Kotlin 侧直接映射 Android API 更自然

缺点:

- 要同步改 `aios-spec`、`aios-agent`、`aios-core`、测试和文档

## 你可以优先修改哪些地方

下面这部分按“最值得动、改动收益最高”的顺序列出。

### 1. 修改动作语义定义

优先级: **最高**

建议修改:

- [`crates/aios-spec/src/intent.rs`](../../../crates/aios-spec/src/intent.rs)

可以改什么:

- 为现有 `ActionType` 注释改成 Android 公开 API 可实现的含义
- 如果采用方案 B，在这里直接重构动作枚举
- 视需要给 `SuggestedAction` 补充更明确的 `target` 约定，例如包名、`Uri`、work tag、notification route

为什么先改这里:

- 这里是动作协议源头
- 不先收紧语义，后面的 executor 实现会一直和文档打架

### 2. 修改 `aios-action` 的执行边界

优先级: **最高**

建议修改:

- [`crates/aios-action/src/lib.rs`](../../../crates/aios-action/src/lib.rs)

可以改什么:

- 把当前注释里的 `/proc`、`oom_score_adj`、`process_madvise` 描述改成 Android-safe 版本
- 保留桌面 / WSL 下的 stub executor 用于 replay 和测试
- 把“真实执行”定义成平台适配层接口，而不是在 Rust 里硬写 Linux 内核路径

推荐方向:

- Rust 侧输出 `AuthorizedAction`
- Android 侧通过 bridge 执行真正的公开 API 调用
- Rust 侧只接收 `ActionResult`

### 3. 新增 Android 侧动作桥接

优先级: **高**

建议修改:

- `apps/android-collector/` 下新增一个动作执行入口

可以改什么:

- 新建 Kotlin 侧 `ActionExecutorBridge`
- 为 `AuthorizedAction` 到 Android API 的映射建立单独模块
- 先只实现 2 到 3 个低风险动作，例如:
  - 预取自己可访问的文件或网络资源
  - 调度 `WorkManager` expedited work
  - 发出预测通知，等待用户点击再进入目标 app

为什么值得先做:

- 这是“真动作”能落地的关键
- 也最符合可行性分析里“Kotlin 负责 Android API 调用，Rust 负责审核”的分工

### 4. 修改 `PolicyEngine` 的动作白名单与风险说明

优先级: **高**

建议修改:

- `crates/aios-core/` 中与动作授权相关的逻辑和测试

可以改什么:

- 调整哪些 route 允许哪些动作
- 对需要用户可感知的动作单独设风险或约束
- 对“必须用户点击才继续”的动作加显式说明

为什么要改:

- 如果动作语义收紧了，授权逻辑也要跟着收紧
- 否则云端仍可能返回“文档上不可实现”的动作组合

### 5. 修改 agent 侧动作生成提示

优先级: **中**

建议修改:

- `crates/aios-agent/` 中 rule-based / cloud backend 的动作输出约束

可以改什么:

- 禁止生成“预热第三方 App 进程”这类不落地动作
- 更偏向生成“可访问资源预取”“自身任务调度”“通知引导”这几类动作
- 给云端 prompt / schema 增加 Android 能力边界说明

### 6. 修改测试与回放样例

优先级: **中**

建议修改:

- `crates/aios-action/tests/`
- `crates/aios-core/tests/`
- `crates/aios-agent/tests/`
- `data/traces/` 与 replay 用例

可以改什么:

- 把当前围绕 syscall stub 的断言改成围绕 Android-safe 语义的断言
- 新增“后台不允许直接拉起 UI”“只能处理自身资源”“无权限时回退 NoOp”这类用例

## 当前阶段不建议优先做的修改

在当前项目边界下，下面这些方向**不建议继续作为主线动作实现**:

- 在 `aios-action` 中实现第三方 App 的 zygote 预热
- 实现 `/proc/pid/oom_score_adj` 来保活别的进程
- 实现 `/proc/pid/reclaim` 或 `process_madvise()` 以释放第三方进程内存
- 依赖 root、Shizuku、system image 或自定义 ROM 作为动作层前提
- 用后台启动 Activity 伪装成“预热”

这些路线可以留在长期研究或 system 版分支里，但不适合作为当前 Android MVP 的主交付。

## 建议的最小落地顺序

1. 先选方案 A 或方案 B，统一动作语义。
2. 收紧 `aios-spec` 和 `aios-action` 注释，停止把动作层描述成 Linux syscall executor。
3. 在 Android 侧补一个最小 bridge，先做 2 到 3 个低风险动作。
4. 再同步调整 `PolicyEngine`、agent 输出和测试。

如果目标是“尽快做出可演示版本”，建议优先实现下面三种动作:

1. `PrefetchAccessibleContent`
2. `ScheduleExpeditedWork`
3. `PostPredictionNotification`

这三类动作:

- 都能用公开 API 落地
- 用户可解释性强
- 最符合“低风险优化”的项目定位

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
