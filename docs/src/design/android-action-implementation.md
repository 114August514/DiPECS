# Android 动作实现手册

> 日期: 2026-06-06  
> 范围: 基于 [Android 动作能力边界](android-action-boundary.md) 的结论，进一步细化每类动作在 DiPECS 中如何具体实现。

## 目标

这份文档不再讨论“能不能做”，而是讨论“如果要做，具体怎么做”。

覆盖范围:

1. 公开 API 下最值得实现的动作
2. 每类动作需要的 Android 能力、权限和执行步骤
3. 在当前仓库里建议修改哪些文件
4. 当前协议哪里够用，哪里不够用

默认前提:

- 继续遵守“仅使用 Android 官方公开 API，API >= 33”的项目约束
- 继续遵守“本地只做低风险、可审计、可关闭优化”的项目边界
- Rust 侧负责授权与审计，Kotlin 侧负责实际 Android API 调用

## 先看当前仓库能接在哪

现有 Android App 里已经有几个天然适合作为动作落点的入口:

- [`MainActivity.kt`](../../../apps/android-collector/app/src/main/java/com/dipecs/collector/MainActivity.kt): 适合加“手动触发动作”“动作调试面板”“权限状态展示”
- [`CollectorForegroundService.kt`](../../../apps/android-collector/app/src/main/java/com/dipecs/collector/services/CollectorForegroundService.kt): 适合作为长期运行、调度 Work、发通知、执行轻量后台动作的入口
- [`CollectorPreferences.kt`](../../../apps/android-collector/app/src/main/java/com/dipecs/collector/storage/CollectorPreferences.kt): 适合保存动作开关、目标 URI、通知 channel 配置、最近一次动作时间
- [`CloudUploader.kt`](../../../apps/android-collector/app/src/main/java/com/dipecs/collector/net/CloudUploader.kt): 这里的网络发送模式可以直接借鉴到“网络预取型动作”
- [`AndroidManifest.xml`](../../../apps/android-collector/app/src/main/AndroidManifest.xml): 权限、Service、Foreground Service type 都在这里接

当前 Manifest 已经具备的基础条件:

- `FOREGROUND_SERVICE`
- `FOREGROUND_SERVICE_DATA_SYNC`
- `INTERNET`
- `ACCESS_NETWORK_STATE`
- `POST_NOTIFICATIONS`

这意味着 “前台服务 + 网络预取 + 通知引导” 这条线已经比别的动作更容易先落地。

## 当前协议的一个现实限制

当前 `SuggestedAction` 只有三个字段:

- `action_type`
- `target: Option<String>`
- `urgency`

见 [`crates/aios-spec/src/intent.rs`](../../../crates/aios-spec/src/intent.rs)。

这意味着如果你现在就开始做 Android 动作，有两个方案:

### 方案 A: 先不改协议

把 `target` 约定成带前缀的字符串，例如:

- `uri:content://...`
- `url:https://...`
- `pkg:com.example.app`
- `work:prefetch_recent_feed`
- `notif:open_chat_prediction`

优点:

- 改动最小
- 能先做 MVP

缺点:

- 类型不够强
- Kotlin / Rust 两端都要靠字符串约定解析

### 方案 B: 给动作补参数

例如把 `SuggestedAction` 扩成:

```rust
pub struct SuggestedAction {
    pub action_type: ActionType,
    pub target: Option<String>,
    pub urgency: ActionUrgency,
    pub params: Option<serde_json::Value>,
}
```

优点:

- 更适合 Android 动作
- 能自然表达 URI、headers、notification route、work constraints

缺点:

- 需要同步改 `spec`、`agent`、`core`、测试和文档

如果你现在想尽快做出第一版，我建议先走 **方案 A**。

## 动作 1: 预取可访问内容

这是当前最值得最先落地的动作。

### 目标

在用户下一步高概率会访问某个内容时，提前把当前应用**有权限访问**的内容准备好。

适合的目标:

- App 自己的文件
- 用户通过 SAF 授权的文档 URI
- `MediaStore` 可访问内容
- 网络资源，例如会话摘要、缩略图、最近文档元数据、推荐列表

### Android 侧具体做法

#### 路线 A: 预取网络资源

适用场景:

- 最稳
- 不涉及额外存储授权
- 很适合你现在这个 collector app 的现状

执行步骤:

1. Rust 输出 `AuthorizedAction { action_type = PrefetchFile, target = Some("url:https://...") }`
2. Kotlin bridge 解析 `url:` 前缀
3. 用 `WorkManager` 或单线程 executor 发起 HTTP 请求
4. 把结果写入 app-specific cache
5. 记录成功/失败、缓存路径、字节数、耗时

建议放置位置:

- 新增 `apps/android-collector/.../actions/PrefetchWorker.kt`
- 新增 `apps/android-collector/.../actions/ActionExecutorBridge.kt`
- 可复用 `CloudUploader.kt` 里的 HTTP 发送模式

缓存建议:

- `context.cacheDir`
- `context.externalCacheDir`
- 如果内容要长期保留，再放 `getExternalFilesDir()`

#### 路线 B: 预取 SAF URI

适用场景:

- 用户明确授予过文档访问
- 你想对“下一个文件操作”做演示

执行步骤:

1. 在 `MainActivity` 里加一个“选择文档/目录”按钮
2. 通过 `ACTION_OPEN_DOCUMENT` 或 `ACTION_OPEN_DOCUMENT_TREE` 让用户选文件
3. 调用 `ContentResolver.takePersistableUriPermission()`
4. 把 `Uri` 存到 `CollectorPreferences`
5. 当动作触发时，用 `ContentResolver.openFileDescriptor()` / `openInputStream()` 读取内容
6. 可选地复制到 app cache 或只读取 metadata

建议放置位置:

- `MainActivity.kt`: 选择 URI + 保存授权
- 新增 `actions/UriPrefetcher.kt`: 读取 URI 并缓存

适合记录到 `ActionResult` 的内容:

- `target=uri:...`
- `success`
- `error`
- `latency_us`

### 需要的 Android 能力

- 网络预取: `INTERNET`
- 网络约束判断: `ACCESS_NETWORK_STATE`
- SAF 预取: 无额外危险权限，但需要用户交互选文件/目录

### 推荐的 MVP 版本

先只做:

- `url:https://...`
- 缓存到 `cacheDir`
- 成功后写一条 internal event 到 trace

这是最短路径。

## 动作 2: 调度加急后台任务

这个动作是 `KeepAlive` 在公开 API 下最合理的替代。

### 目标

保证 DiPECS 自己的后台工作尽快跑起来，而不是试图保活别的进程。

适合的任务:

- 预取网络资源
- 上传最近 trace
- 刷新预测缓存
- 做轻量分析或批量整理

### Android 侧具体做法

#### 路线 A: WorkManager expedited work

执行步骤:

1. Kotlin 侧定义 `CoroutineWorker` 或 `Worker`
2. 构造 `OneTimeWorkRequest`
3. 调用 `setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)`
4. 用 `enqueueUniqueWork()` 提交唯一任务
5. 用 work name 避免同类任务堆积

推荐 work name:

- `prefetch_recent_feed`
- `upload_recent_trace`
- `refresh_prediction_cache`

建议放置位置:

- 新增 `actions/work/ActionWorkScheduler.kt`
- 新增 `actions/work/PrefetchWorker.kt`
- 新增 `actions/work/UploadWorker.kt`

为什么推荐 WorkManager:

- 它适合“app 离开前台后仍要跑”的可靠任务
- 支持约束条件
- 支持唯一任务
- 比自己手搓轮询更符合 Android 背景任务模型

#### 路线 B: JobScheduler

适用场景:

- 你需要直接利用 `JobInfo.Builder.setPrefetch(true)`
- 你希望更贴近系统原生 job 模型

执行步骤:

1. 定义 `JobService`
2. 构造 `JobInfo.Builder`
3. 设约束，例如网络类型、充电状态
4. 对预取任务调用 `setPrefetch(true)`
5. 提交给 `JobScheduler`

建议:

- 如果只是 MVP，不必比 WorkManager 更早做
- 真要做“系统风格的 prefetch 任务”，这条更适合第二阶段

### 需要的 Android 能力

- `WorkManager` 依赖 Jetpack
- `JobScheduler` 无特殊危险权限
- 如果任务需要网络，则继续依赖 `INTERNET` / `ACCESS_NETWORK_STATE`

### 在仓库里具体改哪

- `apps/android-collector/app/build.gradle*`: 加 `androidx.work:work-runtime-ktx`
- `AndroidManifest.xml`: 如需自定义 `JobService` 或额外 `Service`，在这里声明
- `CollectorForegroundService.kt`: 可以把“Upload Recent Events Now”重构成触发 Worker，而不是直接发网络请求

### 当前 `ActionType` 怎么映射

如果暂时不改协议:

- `KeepAlive` -> “调度 DiPECS 自己的 expedited work”
- `PrefetchFile` -> “调度特定 prefetch worker”

## 动作 3: 用户可见的前台服务

这个动作是“需要持续跑一段时间”的版本。

### 目标

在用户可感知的前提下，执行持续的同步、预取或导出任务。

适合的任务:

- 批量同步近期 trace
- 批量预取一组资源
- 导出动作日志
- 临时诊断会话

### Android 侧具体做法

当前仓库已经有一条能跑的路径:

- [`CollectorForegroundService.kt`](../../../apps/android-collector/app/src/main/java/com/dipecs/collector/services/CollectorForegroundService.kt)
- `AndroidManifest.xml` 中已声明 `foregroundServiceType="dataSync"`

因此这项不是从零开始。

#### 具体操作步骤

1. 复用 `CollectorForegroundService`
2. 新增动作 intent，例如:
   - `ACTION_PREFETCH_NOW`
   - `ACTION_RUN_DIAGNOSTIC`
   - `ACTION_EXPORT_AND_UPLOAD`
3. 在 `onStartCommand()` 中分支处理
4. 前台通知里展示任务说明和停止入口
5. 任务完成后 `stopForeground()` / `stopSelf()`

### 为什么不要滥用

前台服务不是保活总开关。

使用条件应当是:

- 用户能感知到
- 任务持续时间明显长于普通后台 work
- 你愿意让系统通知栏长期显示一条状态

### 建议放置位置

- 直接扩展 `CollectorForegroundService.kt`
- 如动作类型变多，再拆一个 `ActionForegroundService.kt`

### 当前仓库最小改法

最省事的方式是:

1. 保留现有 `CollectorForegroundService`
2. 多加几个 action 常量
3. 加一个 `Notification` action button，比如“Stop”
4. 加 trace 记录

## 动作 4: 通过通知引导用户进入目标应用

这是 `PreWarmProcess` 在公开 API 边界里最现实的替代之一。

### 目标

不直接后台拉起目标 App，而是在用户看到通知并点击后，进入 DiPECS 自己的界面或目标跳转页，再由用户继续。

适合的场景:

- “你可能接下来要看某条通知”
- “你刚收到某个会话消息，是否现在打开？”
- “预取已完成，点击继续”

### Android 侧具体做法

#### 路线 A: 通知打开 DiPECS 自己的 Activity

执行步骤:

1. 创建 notification channel
2. 构造 explicit `Intent(this, MainActivity::class.java)`
3. 用 `PendingIntent.getActivity(...)`
4. 通知点击后进入 app 内一个“建议动作页”
5. 用户二次确认后再跳转目标 app 或执行后续步骤

这个路径最稳，也最好解释。

#### 路线 B: 通知 action button

执行步骤:

1. 主点击仍然进入 `MainActivity`
2. `addAction()` 增加如“稍后提醒”“立即上传”“打开建议页”
3. action 可以发 `BroadcastReceiver` 或 `Service` intent

这种方式适合做低打扰交互。

### 当前仓库里怎么接

你已经有前台通知创建逻辑:

- [`CollectorForegroundService.kt`](../../../apps/android-collector/app/src/main/java/com/dipecs/collector/services/CollectorForegroundService.kt)

所以现在可以直接复用以下能力:

- `createNotificationChannel()`
- `PendingIntent.getActivity()`
- `setContentIntent()`

建议新增:

- `PredictionNotificationManager.kt`
- 专门管理“预测通知”而不是和 collector 常驻通知混用

### 需要的 Android 能力

- Android 13+ 运行时通知权限 `POST_NOTIFICATIONS`
- notification channel
- explicit intent

### 协议建议

如果先不改协议，可把 `target` 定义成:

- `pkg:com.ss.android.lark`
- `notif:conversation_prediction`

通知点击后先回到 DiPECS，由 DiPECS 决定怎么解释这个 target。

## 动作 5: 释放自身缓存和资源

这是 `ReleaseMemory` 在 Android 公开 API 下最靠谱的落地方式。

### 目标

不是“释放别人的内存”，而是:

- 丢弃预取缓存
- 取消不必要工作
- 释放 UI / bitmap /索引 /会话缓存
- 让 DiPECS 自己更容易被系统保留在缓存态

### Android 侧具体做法

#### 路线 A: 显式清理

执行步骤:

1. 删除 `cacheDir` 下可重建文件
2. 删除 `externalCacheDir` 下可重建文件
3. 取消对应 unique work
4. 停掉非必要 foreground service
5. 写 trace

适合做成一个明确动作:

- `ReleaseMemory`
- `target = Some("cache:prefetch")`

#### 路线 B: 响应 `onTrimMemory()`

执行步骤:

1. 在 `Application`、`Service` 或关键组件实现 `ComponentCallbacks2`
2. 收到 `TRIM_MEMORY_UI_HIDDEN` 时释放 UI 相关资源
3. 收到 `TRIM_MEMORY_BACKGROUND` 时释放可重建缓存
4. 把释放行为记录为 internal event

这个更像系统信号驱动，而不是 AI 直接下命令，但非常适合补足“真实动作”。

### 当前仓库里怎么接

建议新增:

- `DipecsCollectorApp.kt`: 统一接 `onTrimMemory()`
- `actions/CacheTrimmer.kt`

也可以先在:

- `CollectorForegroundService.kt`

里做最小版的缓存清理调用。

### 需要的 Android 能力

- 无危险权限
- 只操作自己 app 的缓存目录

## 动作 6: 预热自身 isolated service / app zygote

这是 `PreWarmProcess` 最贴近原意、但也最不适合第一个做的版本。

### 目标

不是预热第三方 App，而是预热 DiPECS 自己的隔离服务或共享初始化内容。

### Android 侧具体做法

1. 在 `<application>` 上配置 `android:zygotePreloadName`
2. 实现一个 `ZygotePreload` 类
3. 在某个 `<service>` 上设置:
   - `android:isolatedProcess="true"`
   - `android:useAppZygote="true"`
4. 首次启动该 isolated service 时，系统启动 application zygote 并调用 `doPreload()`
5. 把可共享的初始化内容放进 `doPreload()`

适合预热的内容:

- 类加载
- 本地索引初始化
- 轻量模型文件映射
- 常用配置解析

不适合放进去的内容:

- 与用户会话强绑定的动态状态
- 重 I/O 或网络请求
- 任何第三方 App 进程控制逻辑

### 为什么我不建议你先做它

- 它更偏系统优化技巧
- 你的当前 app 结构还没有 isolated service 需求
- 比起 WorkManager / notification / cache，展示收益没那么直接

因此它更适合:

- 第二阶段
- 你确认要长期保留一个 Android 侧动作执行器之后

## 推荐的仓库修改顺序

如果你想按最小风险推进，我建议这样改。

### 第一批: 先做可演示动作

修改点:

- 新增 `apps/android-collector/.../actions/ActionExecutorBridge.kt`
- 新增 `apps/android-collector/.../actions/PrefetchWorker.kt`
- 新增 `apps/android-collector/.../actions/PredictionNotificationManager.kt`
- 轻改 `CollectorForegroundService.kt`

先实现:

1. 网络资源预取
2. WorkManager expedited work
3. 预测通知
4. 自身缓存清理

### 第二批: 收紧协议与说明

修改点:

- [`crates/aios-spec/src/intent.rs`](../../../crates/aios-spec/src/intent.rs)
- [`crates/aios-spec/src/traits/executor.rs`](../../../crates/aios-spec/src/traits/executor.rs)
- [`crates/aios-action/src/lib.rs`](../../../crates/aios-action/src/lib.rs)

建议动作:

- 先修改注释和语义
- 如果愿意，再给 `SuggestedAction` 增加 `params`

### 第三批: 联动策略层

修改点:

- `crates/aios-core/` 中 `PolicyEngine`
- `crates/aios-agent/` 的 rule-based / cloud backend

要做的事:

- 不再生成“预热第三方 App 进程”这种动作
- 改成输出 Android-safe 动作目标
- 测试回放同步改断言

## 每类动作的最小 ActionResult 建议

虽然现在 `ActionResult` 还比较简单，但先约定日志内容很有帮助。

### 预取类

- `action_type=PrefetchFile`
- `target=url:...` 或 `target=uri:...`
- `success=true/false`
- `error`
- `latency_us`

### 调度类

- `action_type=KeepAlive`
- `target=work:prefetch_recent_feed`
- `success=true/false`
- `error`
- `latency_us`

### 通知类

- `action_type=PreWarmProcess`
- `target=pkg:...` 或 `notif:...`
- `success=true/false`
- `error`
- `latency_us`

### 清理类

- `action_type=ReleaseMemory`
- `target=cache:prefetch`
- `success=true/false`
- `error`
- `latency_us`

## 最小可实现组合

如果你只想先做一条完整链路，我建议选这个组合:

1. 云端或 rule-based 生成 `PrefetchFile(target="url:https://...")`
2. Kotlin bridge 把它转成 `WorkManager` expedited work
3. Worker 把结果写入 `cacheDir`
4. 发送一条预测通知，点击进入 `MainActivity`
5. 记录 `ActionResult`

这样你就能展示:

- 有真实动作
- 用的是公开 Android API
- 不越权
- 能回放和审计

## 参考资料

- Android Developers: [ZygotePreload](https://developer.android.com/reference/android/app/ZygotePreload)
- Android Developers: [Task scheduling / WorkManager](https://developer.android.com/develop/background-work/background-tasks/persistent)
- Android Developers: [Define work requests / expedited work](https://developer.android.com/guide/background/persistent/getting-started/define-work)
- Android Developers: [`JobInfo.Builder.setPrefetch(boolean)`](https://developer.android.com/reference/android/app/job/JobInfo.Builder#setPrefetch(boolean))
- Android Developers: [Foreground services overview](https://developer.android.com/develop/background-work/services/fgs)
- Android Developers: [Restrictions on starting a foreground service from the background](https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start)
- Android Developers: [Foreground service types](https://developer.android.com/develop/background-work/services/fgs/service-types)
- Android Developers: [Notification runtime permission](https://developer.android.com/guide/topics/ui/notifiers/notification-permission)
- Android Developers: [Create and manage notification channels](https://developer.android.com/develop/ui/views/notifications/channels)
- Android Developers: [Start an Activity from a Notification](https://developer.android.com/develop/ui/views/notifications/navigation)
- Android Developers: [Background activity launch restrictions](https://developer.android.com/guide/components/activities/secure-bal)
- Android Developers: [Access app-specific files](https://developer.android.com/training/data-storage/app-specific)
- Android Developers: [Access documents and other files from shared storage](https://developer.android.com/training/data-storage/shared/documents-files)
- Android Developers: [`ContentResolver`](https://developer.android.com/reference/android/content/ContentResolver)
- Android Developers: [`ComponentCallbacks2`](https://developer.android.com/reference/android/content/ComponentCallbacks2.html)
