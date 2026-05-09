# Kotlin 编码规范

> 目标：写出让 Java 程序员也能一眼看懂的 Kotlin 代码。
> 每条规则都有 **Why** 和 **Example**，而非罗列条款。

---

## 1. 总则

### 1.1 Kotlin 是 Android 项目的一等公民

本项目 Android 采集层使用 Kotlin。和 Rust 核心一样，Kotlin 代码同样需要经过 code review、lint 检查、自动化测试。不要因为 Android 侧代码量少就降低标准。

### 1.2 利用编译器

Kotlin 的类型系统、空安全、智能转换是你最好的 reviewer。能在编译期解决的问题，不要留到运行时。

- **优先使用 `val`**，仅在确实需要重新赋值时才用 `var`。
- **禁止 `!!`**——如果你的代码需要 `!!` 才能编译，说明你的空安全设计有问题。
- **善用 `when` 表达式**——编译器会检查穷举性，帮你发现遗漏的分支。

### 1.3 一致性优于个人偏好

如果你在改一个已有文件，请保持和周围代码一致的风格（如花括号换行、lambda 参数命名）。如果你发现整个项目都不一致，提一个单独的 cleanup commit。

---

## 2. 格式

### 2.1 自动化工具为唯一标准

- **使用 `ktlint` 处理所有格式问题**。不要手动调整缩进、换行、空格。
- 在项目根目录放置 `.editorconfig`，将所有风格讨论收敛到这一个文件。
- 每个 commit 前运行 `./gradlew ktlintCheck`。

```properties
# .editorconfig（推荐配置）
[*.kt]
indent_size = 4
indent_style = space
max_line_length = 120
ij_kotlin_allow_trailing_comma = true
ij_kotlin_allow_trailing_comma_on_call_site = true
```

**Why**: 格式问题不值得 code review 时讨论。把决策权交给 `ktlint`，把认知资源留给逻辑和架构。

### 2.2 缩进与换行

- **缩进 4 空格**，不使用 Tab。与 [Google Kotlin Style Guide](https://developer.android.com/kotlin/style-guide) 保持一致。
- 行宽上限 120 字符。
- 如果某个表达式换行后仍然难以阅读，那就说明它应该被提取为一个有名字的变量或函数。

### 2.3 尾随逗号

**启用尾随逗号**。多行参数/属性列表中，最后一项后面加逗号：

```kotlin
// 好：尾随逗号让 diff 更干净，重排更安全
val event = CollectorEvent(
    timestampMs = now,
    source = "usage_stats",
    eventType = eventType,
    packageName = event.packageName,
    className = event.className,
    deviceContext = snapshot(context),
    rawPayload = JSONObject()
        .put("usageEventType", event.eventType)
        .put("configuration", event.configuration?.toString()),
)
```

**Why**: 尾随逗号让添加/删除/重排参数时只改一行 diff，而不是"上一行加逗号 + 新行"改两行。

---

## 3. 命名

| 元素 | 约定 | 示例 |
| --- | --- | --- |
| 包名 | 全小写，点分隔 | `com.dipecs.collector.model` |
| 类/接口/对象 | `PascalCase` | `CollectorEvent`, `EventStore`, `EventRepository` |
| 函数/方法 | `camelCase`，动词开头 | `startCollection()`, `collectSinceLastPoll()` |
| 属性（val/var） | `camelCase`，名词 | `eventId`, `timestampMs`, `isCharging` |
| 常量（顶层、object、companion object） | `SCREAMING_SNAKE_CASE` | `MAX_RETRY_COUNT`, `DEFAULT_TIMEOUT_MS` |
| 扩展函数/属性 | `camelCase` | `EventStore.append()` 如为扩展函数 |
| 布尔变量 | `is`/`has`/`should` 前缀 | `isScreenOn`, `hasPipe`, `shouldExit` |

### 3.1 命名要精确

```kotlin
// 差
val data = parse(s)
val result = execute(data)

// 好
val event = parseRawNotification(sbn)
val exitCode = execute(event)
```

### 3.2 伴生对象常量

```kotlin
companion object {
    const val ACTION_START = "com.dipecs.collector.action.START"
    const val ACTION_STOP = "com.dipecs.collector.action.STOP"
    const val ACTION_UPLOAD_NOW = "com.dipecs.collector.action.UPLOAD_NOW"

    private const val CHANNEL_ID = "dipecs_collector"
    private const val NOTIFICATION_ID = 1101
    private const val USAGE_POLL_INTERVAL_MS = 5_000L
}
```

**公有的用 `const val`**（编译期常量，没有运行时开销），**私有的也用 `const val`**（保持一致）。

### 3.3 数值字面量

使用下划线分隔千位，提高可读性：

```kotlin
// 好
private const val POLL_INTERVAL_MS = 5_000L
private const val UPLOAD_TIMEOUT_MS = 10_000L
private const val MAX_FILE_SIZE = 1_048_576L

// 差
private const val POLL_INTERVAL_MS = 5000L
```

---

## 4. 代码组织

### 4.1 包结构

```text
com.dipecs.collector/
├── model/           # 数据类：CollectorEvent, DeviceContext, AndroidRawEventMapper
├── collectors/      # 采集器：UsageCollector, DeviceContextCollector
├── storage/         # 存储：EventStore, EventRepository, CollectorPreferences
├── services/        # Android Service：CollectorForegroundService, 无障碍/通知服务
├── net/             # 网络：CloudUploader
└── MainActivity.kt  # 入口（纯 UI 调度，无业务逻辑）
```

**Why**: 包名即架构图。一个新人打开 `com/dipecs/collector/` 目录，应该能从包名推断出系统的组成部分。

### 4.2 一个文件一个核心类型

- 一个 `.kt` 文件暴露一个核心类/对象。
- 与该类型紧密耦合的私有 helper 函数、私有数据类可以放在同一个文件中。

```kotlin
// CloudUploader.kt
object CloudUploader {
    // ...
}

// 与 CloudUploader 紧密耦合的私有类型，可放同文件
private data class HttpResponse(val code: Int, val body: String)
```

### 4.3 类内成员顺序

```kotlin
class CollectorForegroundService : Service() {
    // 1. 属性（val / var / lateinit var）
    private val handler = Handler(Looper.getMainLooper())
    private lateinit var usageCollector: UsageCollector
    private var running = false

    // 2. init 块 / 回调对象
    private val pollRunnable = object : Runnable { ... }

    // 3. 生命周期方法（public）
    override fun onCreate() { ... }
    override fun onStartCommand(...) { ... }
    override fun onDestroy() { ... }
    override fun onBind(...) { ... }

    // 4. 私有方法
    private fun startCollector() { ... }
    private fun stopCollector() { ... }

    // 5. companion object（放最后）
    companion object {
        const val ACTION_START = "..."
        private const val CHANNEL_ID = "..."
    }
}
```

**Why**: 阅读者首先看到"这个类有什么状态"（属性），然后看到"它如何响应生命周期"（public 方法），最后看到实现细节和常量。

### 4.4 可见性

- **默认 `private`**。只有确实需要被外部调用时才不加可见性修饰符（Kotlin 默认 `public`）。
- 每个 `public` 函数都代表一个 API 承诺。写下它之前问自己："如果这个签名要改，我会不会犹豫？"——如果会，它就不该是 `public`。

---

## 5. 空安全

### 5.1 永远不要使用 `!!`

`!!` 是对 Kotlin 类型系统的绕过，等同于在墙上写"这里永远不会空"后祈祷。

```kotlin
// 差：绕过了空安全检查
val pkg = event.packageName!!
val name = pkg.substring(1)

// 好：安全链式处理
val firstChar = event.packageName?.firstOrNull() ?: '?'

// 好：需要非空值时提前检查并及早返回
val pkg = event.packageName ?: return
val name = pkg.substring(1)

// 好：需要非空值时抛明确错误
val pkg = event.packageName
    ?: throw IllegalStateException("packageName must not be null for foreground event")
```

### 5.2 安全调用 + Elvis 操作符

```kotlin
// 链式安全调用
val className = event.className?.takeIf { it.isNotBlank() }

// Elvis 提供默认值
val target = appContext.getExternalFilesDir(null) ?: appContext.filesDir

// 链式操作
deviceContext?.toJson() ?: JSONObject.NULL
```

### 5.3 平台类型——在边界处标注

从 Java API 获取的对象是"平台类型"（可空性未知），必须在第一条语句中标注：

```kotlin
// 好：立即用类型标注收窄
val manager: UsageStatsManager? = context.getSystemService(UsageStatsManager::class.java)
    ?: return

// 好：如果确定不会空，用 ?: return 及早退出
val nm = getSystemService(NotificationManager::class.java) ?: return
```

### 5.4 `lateinit` 的使用标准

`lateinit var` 仅用于"构造后初始化"的场景（如 Android `Service.onCreate()` 中初始化）：

```kotlin
// 可接受：在 onCreate 中初始化
class CollectorForegroundService : Service() {
    private lateinit var usageCollector: UsageCollector

    override fun onCreate() {
        super.onCreate()
        usageCollector = UsageCollector(this)
    }
}

// 差：应该有默认值而非 lateinit
private lateinit var name: String  // 为什么不直接用 val + lazy 或默认值？
```

**Why**: `lateinit` 把"变量是否已初始化"从编译期检查推迟到了运行时。`UninitializedPropertyAccessException` 比 `NullPointerException` 好不了多少。

---

## 6. 数据类

### 6.1 用 `data class` 而非手写 `equals`/`hashCode`

```kotlin
data class CollectorEvent(
    val eventId: String = UUID.randomUUID().toString(),
    val timestampMs: Long = System.currentTimeMillis(),
    val source: String,
    val eventType: String,
    val packageName: String? = null,
    val className: String? = null,
    val windowTitle: String? = null,
    val text: String? = null,
    val action: String? = null,
    val deviceContext: DeviceContext? = null,
    val rawEvent: JSONObject? = null,
    val rawPayload: JSONObject = JSONObject(),
)
```

### 6.2 参数顺序

```kotlin
data class MyType(
    // 1. 必填参数（无默认值）
    val source: String,
    val eventType: String,
    // 2. 可选参数（有默认值）
    val packageName: String? = null,
    val timestampMs: Long = System.currentTimeMillis(),
    // 3. 复杂对象（有默认值）
    val rawPayload: JSONObject = JSONObject(),
)
```

**Why**: 调用者最可能指定的参数放在前面，几乎总是用默认值的放在后面。

### 6.3 禁止用 `data class` 当 Service/Repository

`data class` 只用于数据载体。如果一个类依赖 Context、管理状态、或执行 I/O，它不是数据类——即使它碰巧有一些属性。

```kotlin
// 差：这不是数据类
data class EventStore(val context: Context) {  // Context 不应参与 equals
    fun append(event: CollectorEvent) { ... }
}

// 好：普通 class
class EventStore(context: Context) {
    private val appContext = context.applicationContext
    fun append(event: CollectorEvent) { ... }
}
```

---

## 7. 表达式与控制流

### 7.1 `when` 替代 `if-else` 链

```kotlin
// 差
fun eventName(eventType: Int): String {
    if (eventType == UsageEvents.Event.MOVE_TO_FOREGROUND) return "move_to_foreground"
    else if (eventType == UsageEvents.Event.MOVE_TO_BACKGROUND) return "move_to_background"
    else if (eventType == UsageEvents.Event.SCREEN_INTERACTIVE) return "screen_interactive"
    else return "usage_event_$eventType"
}

// 好：when 表达式让编译器检查穷举性
fun eventName(eventType: Int): String = when (eventType) {
    UsageEvents.Event.MOVE_TO_FOREGROUND -> "move_to_foreground"
    UsageEvents.Event.MOVE_TO_BACKGROUND -> "move_to_background"
    UsageEvents.Event.SCREEN_INTERACTIVE -> "screen_interactive"
    UsageEvents.Event.SCREEN_NON_INTERACTIVE -> "screen_non_interactive"
    UsageEvents.Event.KEYGUARD_SHOWN -> "keyguard_shown"
    UsageEvents.Event.KEYGUARD_HIDDEN -> "keyguard_hidden"
    UsageEvents.Event.ACTIVITY_RESUMED -> "activity_resumed"
    UsageEvents.Event.ACTIVITY_PAUSED -> "activity_paused"
    UsageEvents.Event.ACTIVITY_STOPPED -> "activity_stopped"
    else -> "usage_event_$eventType"
}
```

### 7.2 `when` 作为表达式 vs 语句

```kotlin
// 作为表达式（有返回值）——每个分支必须有返回
val result = when (action) {
    ACTION_START -> startCollector()
    ACTION_STOP -> stopCollector()
    ACTION_UPLOAD_NOW -> CloudUploader.uploadRecent(this, reason = "manual")
    else -> START_STICKY
}

// 作为语句（无返回值）——用于副作用
when {
    isForegroundEvent(event.eventType) -> handleForeground(event)
    isBackgroundEvent(event.eventType) -> handleBackground(event)
    event.eventType == SCREEN_INTERACTIVE -> handleScreenOn(event)
}
```

### 7.3 `if` 作为表达式

```kotlin
// Kotlin 的 if 可以返回值
val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
    Notification.Builder(this, CHANNEL_ID)
} else {
    @Suppress("DEPRECATION")
    Notification.Builder(this)
}
```

**Why**: `if`-表达式消除了"先声明可变变量，然后在 if-else 中赋值"的模式。

### 7.4 避免多层嵌套

```kotlin
// 差：箭头式嵌套
fun collect() {
    if (enabled) {
        val manager = getSystemService(...)
        if (manager != null) {
            val events = manager.query(...)
            if (events != null) {
                // 三层缩进后才到实际逻辑
            }
        }
    }
}

// 好：及早返回
fun collect() {
    if (!CollectorPreferences.isUsageEnabled(appContext)) return
    val manager = appContext.getSystemService(UsageStatsManager::class.java) ?: return
    val events = runCatching { manager.queryEvents(start, now) }.getOrNull() ?: return
    // 实际逻辑在这里，零缩进
}
```

---

## 8. 错误处理

### 8.1 `runCatching` 优于 `try-catch`

```kotlin
// 差：try-catch 样板代码
val events: UsageEvents? = try {
    usageStatsManager.queryEvents(start, now)
} catch (e: Exception) {
    null
}
if (events == null) return

// 好：runCatching 链式处理
val events = runCatching { usageStatsManager.queryEvents(start, now) }
    .getOrNull() ?: return

// 更好：需要区分成功/失败时
runCatching {
    postJson(endpoint, payload, bearerToken)
}.onSuccess { response ->
    EventRepository.recordInternal(ctx, "upload_success", "...")
}.onFailure { error ->
    EventRepository.recordInternal(ctx, "upload_failed", error.message ?: "...")
}
```

### 8.2 异常仅用于不可恢复的编程错误

```kotlin
// 可接受：不可恢复的编程错误
val pkg = event.packageName
    ?: throw IllegalStateException("packageName must not be null for foreground event")

// 差：用异常控制正常业务流程
fun upload(endpoint: String) {
    if (endpoint.isBlank()) throw IllegalArgumentException("endpoint is blank")
    // 调用者被迫用 try-catch 来处理这种"正常"情况
}

// 好：对于可恢复的错误用 Result 或 null 返回
fun upload(endpoint: String): Result<HttpResponse> {
    if (endpoint.isBlank()) return Result.failure(IllegalArgumentException("endpoint is blank"))
    // ...
}
```

### 8.3 协程中的错误隔离

```kotlin
// 在协程作用域中使用 supervisorScope 隔离错误
suspend fun collectAll() = supervisorScope {
    launch { collectUsage() }
    launch { collectDeviceContext() }
    launch { upload() }
    // 三个子协程互不影响——collectUsage 失败不影响 collectDeviceContext
}

// 不要用 coroutineScope，它会在任一子协程失败时取消所有兄弟
```

---

## 9. 协程

### 9.1 使用结构化并发

```kotlin
// 好：绑定到生命周期
class CollectorViewModel : ViewModel() {
    fun start() {
        viewModelScope.launch {
            while (isActive) {
                collect()
                delay(POLL_INTERVAL_MS)
            }
        }
    }
}

// 差：自由漂浮的协程
fun start() {
    GlobalScope.launch {  // 永远不会取消，泄漏
        while (true) {
            collect()
            delay(POLL_INTERVAL_MS)
        }
    }
}
```

### 9.2 `GlobalScope` 禁令

**禁止 `GlobalScope`**。唯一的替代方案：

- ViewModel：`viewModelScope`
- Activity/Fragment：`lifecycleScope`
- Service：自定义 `CoroutineScope`，在 `onDestroy()` 中 `cancel()`
- 纯工具函数：接受 `CoroutineScope` 作为参数

### 9.3 取消协作

```kotlin
// 所有 suspend 函数必须是可取消的
suspend fun processEvents(events: List<CollectorEvent>) {
    for (event in events) {
        // 每次迭代检查协程是否被取消
        ensureActive()
        processOne(event)
    }
}

// 或者使用 yield 让出执行权
suspend fun heavyComputation() {
    repeat(1000) { i ->
        doChunk(i)
        yield()  // 检查取消信号
    }
}
```

### 9.4 线程切换

```kotlin
// 本项目中 Handler + Runnable 模式用于非协程场景
private val handler = Handler(Looper.getMainLooper())
handler.postDelayed(pollRunnable, POLL_INTERVAL_MS)

// 后台任务用 Executors（项目当前做法，见 CloudUploader）
private val executor = Executors.newSingleThreadExecutor()
executor.execute { /* 后台 I/O */ }
```

---

## 10. 作用域函数（`let`/`run`/`apply`/`also`/`takeIf`）

### 10.1 选型指南

| 函数 | `this` 还是 `it` | 返回值 | 最佳适用场景 |
| --- | --- | --- | --- |
| `let` | `it` | lambda 结果 | 可空链式调用（`?.let {}`）、转换后返回 |
| `run` | `this` | lambda 结果 | 对象配置 + 计算返回值 |
| `apply` | `this` | 对象本身 | 对象初始化配置（Builder 模式） |
| `also` | `it` | 对象本身 | 副作用（日志、验证）不改变对象 |
| `takeIf` | `it` | 对象本身或 null | 条件过滤 |
| `takeUnless` | `it` | 对象本身或 null | 反向条件过滤 |

### 10.2 示例

```kotlin
// let：可空链式处理
val pkg = event.packageName?.takeIf { it.isNotBlank() }?.let { sanitize(it) }

// apply：配置对象
val payload = JSONObject().apply {
    put("schema", "dipecs.collector.v1")
    put("reason", reason)
    put("events", JSONArray(events))
}

// also：副作用（日志/调试）
val result = compute().also { log("computed: $it") }

// takeIf：条件过滤
val validName = name?.takeIf { it.isNotBlank() } ?: return null
```

### 10.3 不要过度使用

```kotlin
// 差：let 链式调用过长，难以阅读
val result = data?.let { parse(it) }?.let { validate(it) }?.let { transform(it) }
    ?: default

// 好：提前返回，逻辑清晰
val parsed = data?.let { parse(it) } ?: return default
if (!validate(parsed)) return default
return transform(parsed)
```

---

## 11. 单例与伴生对象

### 11.1 `object` 用于无状态单例

```kotlin
// 好：纯函数/委托性质的单例
object EventRepository {
    fun record(context: Context, event: CollectorEvent) {
        EventStore(context).append(event)
    }
}

object CloudUploader {
    fun uploadRecent(context: Context, reason: String = "manual") { ... }
}
```

**Why**: 本项目用 `object` 是因为这些类是无状态的"命名空间函数集合"——不需要依赖注入。如果有状态（如缓存），应该用 `class` + 注入。

### 11.2 伴生对象用于工厂方法和常量

```kotlin
class CollectorEvent(
    val source: String,
    ...
) {
    companion object {
        // 工厂方法
        fun fromNotification(sbn: StatusBarNotification): CollectorEvent = ...

        // 常量
        private const val SCHEMA_VERSION = "dipecs.collector.v1"
    }
}
```

### 11.3 伴生对象放类的最后

类内成员声明按以下顺序：属性 → init 块/回调对象 → 生命周期方法（public） → 私有方法 → companion object（最后）。

---

## 12. Android 特定规范

### 12.1 Activity/Service 的职责分离

- **Activity**：只做 UI 绑定，不做业务逻辑。
- **Service**：生命周期管理 + 调度，具体的采集/存储/上传逻辑委托给专门的类。

```kotlin
// CollectorForegroundService：只管生命周期和调度
override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    when (intent?.action ?: ACTION_START) {
        ACTION_START -> startCollector()
        ACTION_STOP -> stopCollector()
        ACTION_UPLOAD_NOW -> CloudUploader.uploadRecent(this, reason = "manual")
    }
    return START_STICKY
}

// UsageCollector：只管采集逻辑（不知道自己在 Service 中运行）
class UsageCollector(private val context: Context) {
    fun collectSinceLastPoll() { ... }
}
```

### 12.2 Context 使用

- **永远不要持有 Activity Context 的长期引用**。如果需要跨生命周期持有，用 `context.applicationContext`。
- 已经是 `Service`/`Application` 的 this 可以直接传（它们本身就是 application context）。

```kotlin
class EventStore(context: Context) {
    private val appContext = context.applicationContext  // 安全
    // ...
}

// 在 Service 内部传给工具类时
EventRepository.record(this, event)  // Service this 等同于 applicationContext
```

### 12.3 线程模型标注

每个与 Android 系统 API 交互的方法，如果线程模型不显而易见，应加注释：

```kotlin
// 好：线程模型清楚
class UsageCollector(private val context: Context) {
    /**
     * 采集自上次轮询以来的 UsageEvents 并写入 EventStore。
     *
     * 此方法可被 Handler post 到主线程调用（API < 28 时 queryEvents 有主线程限制）。
     */
    fun collectSinceLastPoll() { ... }
}

object CloudUploader {
    // 所有网络 I/O 在单线程 executor 中执行，调用者可在任意线程调用
    fun uploadRecent(context: Context, reason: String = "manual") { ... }
}
```

### 12.4 Intent Action 常量

使用包名限定常量避免跨应用冲突。使用 `const val`：

```kotlin
companion object {
    const val ACTION_START = "com.dipecs.collector.action.START"
    const val ACTION_STOP = "com.dipecs.collector.action.STOP"
    const val ACTION_UPLOAD_NOW = "com.dipecs.collector.action.UPLOAD_NOW"
}
```

### 12.5 版本兼容

使用 `Build.VERSION.SDK_INT` 进行版本判断，并用 `@Suppress("DEPRECATION")` 标注已知的弃用 API 调用：

```kotlin
val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
    Notification.Builder(this, CHANNEL_ID)
} else {
    @Suppress("DEPRECATION")
    Notification.Builder(this)
}
```

**Why**: Android Lint 有时会把"有条件的弃用调用"也标记为 warning。`@Suppress` 表示你已经审查过这段代码，弃用版本的分支只在旧设备上执行。

---

## 13. 测试

### 13.1 测试文件位置

```text
app/src/
├── main/java/com/dipecs/collector/
│   └── model/AndroidRawEventMapper.kt
└── test/java/com/dipecs/collector/
    └── model/AndroidRawEventMapperTest.kt
```

### 13.2 测试命名

```kotlin
// 格式：test<method>__<scenario>
@Test
fun testAppTransition__validPackage_returnsEvent() { ... }

@Test
fun testAppTransition__nullPackage_returnsNull() { ... }

@Test
fun testScreenState__allStates_haveValidJson() { ... }
```

### 13.3 纯数据转换——重点测试区

`AndroidRawEventMapper` 这类"输入 → JSONObject"的纯函数是测试价值最高的区域：

```kotlin
@Test
fun testAppTransition() {
    val json = AndroidRawEventMapper.appTransition(
        timestampMs = 1000L,
        packageName = "com.test.app",
        activityClass = "MainActivity",
        transition = "Foreground",
    )
    assertEquals("com.test.app", json.getString("packageName"))
    assertEquals("Foreground", json.getString("transition"))
    assertEquals(1000L, json.getLong("timestampMs"))
}
```

### 13.4 对 Android 系统 API 的隔离

采集器类（如 `UsageCollector`）依赖 `UsageStatsManager` 等系统 API。当前项目通过 `runCatching` 包裹系统调用来实现容错。当测试需求增加时，考虑将系统 API 抽象为接口以便 mock。

---

## 14. 文档与注释

### 14.1 注释哲学

代码告诉你**做了什么**，注释告诉你**为什么这么做**。

以下情况需要注释：

- 非显而易见的算法选择或 workaround
- API 版本兼容的特殊处理逻辑
- `@Suppress` 的原因
- 线程模型说明（该方法预期在哪个线程调用）

以下情况不需要注释：

- 代码本身已经清晰表达的逻辑
- 明显的 `// TODO`——要么修掉，要么提到 Issue 追踪

### 14.2 KDoc

```kotlin
/**
 * 采集自上次轮询以来的 [UsageEvents.Event] 并存储到 [EventStore]。
 *
 * 在 API < 28 时，[UsageStatsManager.queryEvents] 必须在主线程调用，
 * 因此此方法通过 [Handler] 在主线程事件循环中执行。
 *
 * 采集完成后更新 [CollectorPreferences.lastUsageQueryMs] 以避免重复采集。
 */
fun collectSinceLastPoll() { ... }
```

- **每个 `public` 函数都必须有 KDoc**。
- **首行是一个以句号结尾的单句摘要**。
- `[引用]` 使用方括号链接到其他类型/方法。

### 14.3 `@Suppress` 必须注释原因

```kotlin
// 好
@Suppress("DEPRECATION")  // 此分支仅在 API < 26 时执行，旧设备无 NotificationChannel
Notification.Builder(this)

// 差：没有解释为什么 suppress
@Suppress("DEPRECATION")
Notification.Builder(this)
```

---

## 15. 依赖管理

### 15.1 添加依赖前过三关

1. **Android SDK 是否已经提供了？**——检查 `androidx` 和 Jetpack。
2. **依赖本身是否健康？**——检查最近更新时间、下载量、issue 数量。
3. **引入的 transitive dependencies 有多少？**——`./gradlew app:dependencies` 检查。

### 15.2 版本声明

在 `gradle/libs.versions.toml`（如使用 Version Catalog）或 `build.gradle.kts` 中集中管理版本：

```kotlin
// build.gradle.kts
dependencies {
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.1")
}
```

---

## 16. 提交前检查

### 16.1 检查清单

每次开发完成一个小功能后执行：

```bash
# 格式化检查
./gradlew ktlintCheck

# 静态分析
./gradlew detekt

# 编译检查
./gradlew assembleDebug

# 单元测试
./gradlew test
```

### 16.2 Gradle 任务说明

| 命令 | 作用 |
| --- | --- |
| `./gradlew ktlintCheck` | 代码格式检查（等价于 `cargo fmt --check`） |
| `./gradlew detekt` | 静态分析（等价于 `cargo clippy`） |
| `./gradlew test` | 单元测试（等价于 `cargo test`） |
| `./gradlew assembleDebug` | 编译检查（等价于 `cargo check`） |

---

## 17. 快速决策速查表

| 场景 | 选择 |
| --- | --- |
| 可变 vs 不可变 | 默认 `val`，需要重新赋值才用 `var` |
| 可空类型安全解包 | `?.let {}` 或 `?: return`（不用 `!!`） |
| 空安全默认值 | `?:` Elvis 操作符 |
| 平台类型（来自 Java） | 边界处立即标注可空性 `?: return` |
| 多分支选择 | `when` 表达式（不用 `if-else` 链） |
| 错误处理——可恢复 | `runCatching {}.getOrNull()` 或 `Result<T>` |
| 错误处理——不可恢复 | `throw IllegalStateException("why")` |
| 对象初始化配置 | `apply {}` |
| 可空链式转换 | `?.let {}` |
| 副作用（日志/调试） | `also {}` |
| 条件过滤 | `?.takeIf {} ?: default` |
| 无状态工具函数集合 | `object` 单例 |
| 编译期常量 | `const val`（在 `companion object` 或顶层） |
| 数据载体 | `data class` |
| 构造后初始化的依赖 | `lateinit var`（优先考虑 `lazy`） |
| Context 长期持有 | `context.applicationContext` |
| 后台 I/O | `Executors.newSingleThreadExecutor()` 或协程 |
| 协程作用域（ViewModel） | `viewModelScope` |
| 协程作用域（Service） | 自定义 `CoroutineScope`，在 `onDestroy()` 中 cancel |
| 全局协程 | **禁止** `GlobalScope` |
| 版本兼容分支 | `if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.X)` |
| 弃用 API 抑制 | `@Suppress("DEPRECATION")` **加注释说明原因** |
| 服务意图 Action | `const val` + 包名限定前缀 |
| 测试命名 | `test<method>__<scenario>` |
| 往主分支提交 | 通过 ktlintCheck + detekt + test |

---

## 参考资料

- [Google Kotlin Style Guide](https://developer.android.com/kotlin/style-guide)
- [Kotlin Coding Conventions](https://kotlinlang.org/docs/coding-conventions.html)
- [Kotlin 官方文档 · 协程](https://kotlinlang.org/docs/coroutines-guide.html)
- [Android API 指南 · 后台任务](https://developer.android.com/guide/background)
- [ktlint](https://github.com/pinterest/ktlint)
- [detekt](https://detekt.dev/)
