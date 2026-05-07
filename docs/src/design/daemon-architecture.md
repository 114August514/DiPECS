# DiPECS Daemon 架构设计

> **日期**: 2026-05-04
> **定位**: 系统守护进程 (Rust ELF, `/system/bin/dipecsd`), 作为隐私脱敏边界和系统级观测基础设施

---

## 一、架构总览

```
┌────────────────────────────────────────────────────────────┐
│  Decision backends (策略面)                                │
│  RuleBased / LocalEvaluator / CloudLlm / FallbackNoOp       │
│  输入: StructuredContext, 输出: IntentBatch                 │
└──────────────────────────┬─────────────────────────────────┘
                           │ optional HTTPS for CloudLlm
┌──────────────────────────┼─────────────────────────────────┐
│  dipecsd / aios-daemon   │                                  │
│                          │                                  │
│  ┌───────────────────────────────────────────────────────┐ │
│  │                  aios-agent                            │ │
│  │  DecisionRouter: StructuredContext → IntentBatch       │ │
│  │  超时降级, 熔断器, 本地保守策略 fallback                │ │
│  └───────────────────────────┬───────────────────────────┘ │
│                              │                              │
│  ┌───────────────────────────▼───────────────────────────┐ │
│  │                  aios-core                             │ │
│  │  ┌─────────────┐  ┌──────────────┐  ┌──────────────┐ │ │
│  │  │ ActionBus   │  │ PolicyEngine │  │ PrivacyAirGap│ │ │
│  │  │ (事件总线)  │  │ (策略校验)   │  │ (脱敏引擎)   │ │ │
│  │  └─────────────┘  └──────────────┘  └──────────────┘ │ │
│  │  ┌──────────────────────────────────────────────────┐ │ │
│  │  │ TraceEngine (确定性回放 + Golden Trace 验证)      │ │ │
│  │  └──────────────────────────────────────────────────┘ │ │
│  └───────────────────────────┬───────────────────────────┘ │
│                              │                              │
│  ┌───────────────────────────▼───────────────────────────┐ │
│  │                  aios-action                           │ │
│  │  DefaultActionExecutor: AuthorizedAction → ActionResult│ │
│  └───────────────────────────┬───────────────────────────┘ │
│                              │                              │
│  ┌───────────────────────────▼───────────────────────────┐ │
│  │                  aios-collector                        │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │ │
│  │  │ App Source   │  │ ProcReader   │  │ BinderProbe  │ │ │
│  │  │ JSONL/JNI    │  │ (/proc)      │  │ (eBPF)       │ │ │
│  │  └──────────────┘  └──────────────┘  └──────────────┘ │ │
│  └───────────────────────────────────────────────────────┘ │
│                              │                              │
│              Android Kernel (syscalls, Binder, VFS)         │
└─────────────────────────────────────────────────────────────┘
```

**关键设计决策**:
- Daemon 内部是**同步优先** (aios-core 不引入不必要的 async)
- 异步点集中在**系统边界**: collector 读取系统事件、agent 发 HTTPS
- 所有原始数据在 `PrivacyAirGap` 处被截断, 之后只存在脱敏数据

---

## 二、向上的结构化接口

### 2.1 原始事件流 (collector → core)

collector 从 app 侧采集能力或 system 观测入口接收事件, 统一为 `CollectorEnvelope` / `RawEvent`:

```rust
/// 从系统采集的原始事件, 未经脱敏
/// 此类型仅存在于 collector-core 边界内部, 不出 daemon
pub enum RawEvent {
    BinderTransaction(BinderTxEvent),
    ProcStateChange(ProcStateEvent),
    FileSystemAccess(FsAccessEvent),
    NotificationPosted(NotificationRawEvent),
    ScreenState(ScreenStateEvent),
    SystemState(SystemStateEvent),
}

/// Binder 事务事件 (来自 eBPF tracepoint)
pub struct BinderTxEvent {
    pub timestamp_ms: i64,
    pub source_pid: u32,
    pub source_uid: u32,
    pub target_service: String,       // e.g. "notification", "activity", "window"
    pub target_method: String,        // e.g. "enqueueNotificationWithTag"
    pub is_oneway: bool,
    pub payload_size: u32,            // Parcel 大小, 不存内容
}

/// 进程状态变化 (来自 /proc 轮询)
pub struct ProcStateEvent {
    pub timestamp_ms: i64,
    pub pid: u32,
    pub uid: u32,
    pub package_name: Option<String>, // 通过 /proc/pid/cmdline 解析
    pub vm_rss_kb: u64,
    pub vm_swap_kb: u64,
    pub threads: u32,
    pub oom_score: i32,               // 内核 LMK 打分 (越低越不容易被杀)
    pub io_read_mb: u64,              // 累计读
    pub io_write_mb: u64,             // 累计写
    pub state: ProcState,             // Running / Sleeping / Zombie
}

/// 文件系统访问事件 (来自 fanotify)
pub struct FsAccessEvent {
    pub timestamp_ms: i64,
    pub pid: u32,
    pub uid: u32,
    pub path_pattern: String,         // 脱敏: 只保留扩展名
    pub extension: Option<String>,    // "pdf", "docx", "jpg", ...
    pub access_type: FsAccessType,    // OpenRead / OpenWrite / Create / Delete
    pub bytes_transferred: Option<u64>,
}

/// 通知原始事件 (来自 NotificationListenerService, 通过 Binder bridge 传入)
pub struct NotificationRawEvent {
    pub timestamp_ms: i64,
    pub package_name: String,
    pub category: Option<String>,
    pub channel_id: Option<String>,
    pub raw_title: String,            // ⚠️ 含 PII, 仅在此结构体中存在
    pub raw_text: String,             // ⚠️ 含 PII, 仅在此结构体中存在
    pub is_ongoing: bool,
    pub group_key: Option<String>,
    pub has_picture: bool,
}

pub enum ProcState { Running, Sleeping, Zombie, Unknown }
pub enum FsAccessType { OpenRead, OpenWrite, Create, Delete }
```

### 2.2 脱敏后的事件 (core 内部使用)

`PrivacyAirGap` 将 `RawEvent` 转化为 `SanitizedEvent`, 这是原始数据的**最后存在形式**:

```rust
/// 脱敏后的事件
/// 这是 daemon 内部的统一数据模型, 不再包含任何 PII
pub struct SanitizedEvent {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub event_type: SanitizedEventType,
    /// 数据来源能力等级
    pub source_tier: SourceTier,
    /// 关联的 app package
    pub app_package: Option<String>,
    /// 关联的 uid
    pub uid: Option<u32>,
}

pub enum SanitizedEventType {
    /// 应用间交互 (从 Binder 事务推断)
    InterAppInteraction {
        source_package: Option<String>,
        target_service: String,
        interaction_type: InteractionType,
    },
    /// 通知
    Notification {
        source_package: String,
        category: Option<String>,
        channel_id: Option<String>,
        title_hint: TextHint,
        text_hint: TextHint,
        semantic_hints: Vec<SemanticHint>,
        is_ongoing: bool,
        group_key: Option<String>,
    },
    /// 进程资源状态
    ProcessResource {
        pid: u32,
        package_name: Option<String>,
        vm_rss_mb: u32,
        vm_swap_mb: u32,
        thread_count: u32,
        oom_score: i32,
    },
    /// 文件系统活动
    FileActivity {
        package_name: Option<String>,
        extension_category: ExtensionCategory,
        activity_type: FsActivityType,
        /// 是否为已知的热点文件
        is_hot_file: bool,
    },
    /// 屏幕状态
    Screen {
        state: ScreenState,
    },
    /// 系统状态快照
    SystemStatus {
        battery_pct: Option<u8>,
        is_charging: bool,
        network: NetworkType,
        ringer_mode: RingerMode,
        location_type: LocationType,
        headphone_connected: bool,
    },
}

// ===== 脱敏辅助类型 =====

pub struct TextHint {
    pub length_chars: usize,
    pub script: ScriptHint,
    pub is_emoji_only: bool,
}

pub enum ScriptHint { Latin, Hanzi, Cyrillic, Arabic, Mixed, Unknown }

pub enum SemanticHint {
    FileMention,
    ImageMention,
    AudioMessage,
    LinkAttachment,
    UserMentioned,
    CalendarInvitation,
    /// 含有金融/交易相关关键词
    FinancialContext,
    /// 含有验证码相关关键词
    VerificationCode,
}

pub enum InteractionType {
    /// App A 发了一个通知
    NotifyPost,
    /// App A 启动/调起了 App B
    ActivityLaunch,
    /// App A 通过 ShareSheet 分享内容到 App B
    ShareIntent,
    /// App A 绑定了 App B 的服务
    ServiceBind,
}

pub enum ExtensionCategory {
    Document,   // pdf, doc, docx, xls, xlsx, ppt, pptx, txt, md
    Image,      // jpg, jpeg, png, gif, webp, heic
    Video,      // mp4, mov, avi, mkv
    Audio,      // mp3, wav, aac, flac, ogg
    Archive,    // zip, rar, 7z, tar, gz
    Code,       // apk, py, js, rs, cpp, java, kt, so
    Other,
    Unknown,
}

pub enum FsActivityType { Read, Write, Create, Delete }

pub enum ScreenState { Interactive, NonInteractive, KeyguardShown, KeyguardHidden }

pub enum NetworkType { Wifi, Cellular, Offline, Unknown }

pub enum RingerMode { Normal, Vibrate, Silent }

pub enum LocationType { Home, Work, Commute, Unknown }

pub enum SourceTier {
    /// Tier 0: 公开 API (UsageStats, NotificationListener, 系统广播)
    PublicApi = 0,
    /// Tier 1: daemon 级系统访问 (/proc, Binder tracepoint, fanotify)
    Daemon = 1,
}
```

### 2.3 上下文窗口 (core → agent → Cloud LLM)

`SanitizedEvent` 按时间窗口聚合, 形成发送给云端的结构化上下文:

```rust
/// 时间窗口内的脱敏上下文
/// 这是 aios-agent 发送给 Cloud LLM 的唯一数据格式
pub struct StructuredContext {
    /// 窗口唯一 ID
    pub window_id: String,
    /// 窗口起始时间 (epoch ms)
    pub window_start_ms: i64,
    /// 窗口结束时间 (epoch ms)
    pub window_end_ms: i64,
    /// 窗口持续的秒数
    pub duration_secs: u32,
    /// 窗口内的事件序列 (按时间排序, 已脱敏)
    pub events: Vec<SanitizedEvent>,
    /// 窗口聚合摘要 (帮助 LLM 快速理解)
    pub summary: ContextSummary,
}

/// 窗口聚合摘要
pub struct ContextSummary {
    /// 窗口内的前台 app 序列 (按时间)
    pub foreground_apps: Vec<String>,
    /// 收到通知的 app 列表
    pub notified_apps: Vec<String>,
    /// 触发的语义标签汇总
    pub all_semantic_hints: Vec<SemanticHint>,
    /// 文件活动汇总 (扩展名 → 次数)
    pub file_activity: Vec<(ExtensionCategory, u32)>,
    /// 系统状态 (取窗口内的最新值)
    pub latest_system_status: Option<SystemStatusSnapshot>,
    /// 来源能力等级
    pub source_tier: SourceTier,
}

pub struct SystemStatusSnapshot {
    pub battery_pct: Option<u8>,
    pub is_charging: bool,
    pub network: NetworkType,
    pub ringer_mode: RingerMode,
    pub location_type: LocationType,
    pub headphone_connected: bool,
}
```

### 2.4 云端返回 (LLM → agent → core)

```rust
/// 云端 LLM 返回的结构化决策
pub struct IntentBatch {
    /// 请求对应的窗口 ID
    pub window_id: String,
    /// 候选意图列表 (按置信度降序)
    pub intents: Vec<Intent>,
    /// 生成时间
    pub generated_at_ms: i64,
    /// 模型信息
    pub model: String,
}

pub struct Intent {
    /// 意图唯一 ID
    pub intent_id: String,
    /// 意图类型
    pub intent_type: IntentType,
    /// 置信度 (0.0 - 1.0)
    pub confidence: f32,
    /// 风险等级 (由 LLM 判断, 本地二次校验)
    pub risk_level: RiskLevel,
    /// 该意图的推荐动作
    pub suggested_actions: Vec<SuggestedAction>,
    /// LLM 给出的理由摘要 (简短, 不用自然语言, 用标签即可)
    pub rationale_tags: Vec<String>,
}

pub enum IntentType {
    /// 用户将打开某个 app
    OpenApp(String),
    /// 用户将切换到某个 app
    SwitchToApp(String),
    /// 用户将查看某条通知
    CheckNotification(String),
    /// 用户将处理某类文件
    HandleFile(ExtensionCategory),
    /// 用户即将进入某个物理场景 (通勤/到家/到公司)
    EnterContext(LocationType),
    /// 无明确意图, 保持观察
    Idle,
}

pub enum RiskLevel {
    /// 可自动执行
    Low,
    /// 需要轻量确认后执行
    Medium,
    /// 仅建议, 不自动执行
    High,
}

pub struct SuggestedAction {
    pub action_type: ActionType,
    pub target: Option<String>,       // 目标 app package 或其他标识
    pub urgency: ActionUrgency,       // 紧迫度
}

pub enum ActionType {
    /// 预热应用进程 (fork zygote, 不做任何初始化)
    PreWarmProcess,
    /// 预加载热点文件到页缓存
    PrefetchFile,
    /// 保活当前前台进程 (延迟 LMK 回收)
    KeepAlive,
    /// 释放指定进程的非关键内存
    ReleaseMemory,
    /// 不执行任何操作
    NoOp,
}

pub enum ActionUrgency {
    /// 立即执行 (用户可能在 10s 内操作)
    Immediate,
    /// 在空闲时执行 (屏幕关闭、CPU 空闲)
    IdleTime,
    /// 延迟执行
    Deferred,
}
```

---

## 三、Daemon 内部模块通信

```
┌─collector────┐  RawEvent channel   ┌─core──────┐  StructuredContext  ┌─agent──────┐
│ AppSource    │────────────────────→│           │───────────────────→│Decision    │
│ ProcReader   │  (mpsc::Sender)     │PrivacyGap │                    │Router      │
│ BinderProbe  │                     │           │                    │            │
│ SysCollector │                     │ActionBus  │  IntentBatch       │            │
└──────────────┘                     │TraceEngine│←───────────────────│            │
                                     │           │  (oneshot::Sender) │            │
                                     └─────┬─────┘                    └────────────┘
                                         │
                                   ┌─────▼─────┐
                                   │ aios-     │
                                   │ action    │
                                   │ Executor  │
                                   │           │
                                   └───────────┘
```

- collector→core: `tokio::sync::mpsc` channel (bounded, backpressure)
- core→agent: 函数调用 (同步, daemon 装配 core 与 agent)
- agent→core→action: `IntentBatch` 通过 `ActionBus` 派发到 `PolicyEngine`
- PolicyEngine 决定执行的 action, 通过 action executor 发出

---

## 四、隐私脱敏引擎 (PrivacyAirGap) 规范

```rust
/// 隐私脱敏引擎
///
/// 这是 DiPECS 最核心的模块之一。
/// 所有 RawEvent 在此处被转化为 SanitizedEvent,
/// 原始数据 (通知正文、文件名、Binder 参数) 在此之后不可访问。
pub trait PrivacySanitizer {
    /// 对单个原始事件进行脱敏
    fn sanitize(&self, raw: RawEvent) -> SanitizedEvent;

    /// 批量脱敏, 用于窗口聚合场景
    fn sanitize_batch(&self, raw_events: Vec<RawEvent>) -> Vec<SanitizedEvent> {
        raw_events.into_iter().map(|e| self.sanitize(e)).collect()
    }
}

/// 默认实现的关键脱敏规则:
///
/// 1. 通知标题/正文 → TextHint (只保留长度、书写系统、是否纯emoji) + SemanticHints (本地关键词匹配)
/// 2. 文件路径 → ExtensionCategory (只保留扩展名类别)
/// 3. Binder payload → 只保留 service 名和方法名, 丢弃参数
/// 4. /proc 数据 → 已经是系统级指标, 不含 PII, 直接保留
/// 5. 所有原始字符串在 sanitize() 返回后, 通过 ownership 被 drop
```

---

## 五、确定性 Trace 回放

```rust
/// Golden Trace 记录
///
/// 一条 Golden Trace 是在特定时间窗口内:
/// 1. 输入: Vec<RawEvent> (原始事件序列)
/// 2. 脱敏输出: Vec<SanitizedEvent> (脱敏后事件序列)
/// 3. 云端返回: IntentBatch (LLM 决策)
/// 4. 策略决策: Vec<ExecutedAction> (策略引擎的输出和执行结果)
pub struct GoldenTrace {
    pub trace_id: String,
    pub window_start_ms: i64,
    pub window_end_ms: i64,
    pub raw_events: Vec<RawEvent>,
    pub expected_sanitized: Vec<SanitizedEvent>,
    pub expected_intents: IntentBatch,
    pub expected_actions: Vec<ExecutedAction>,
}

/// 回放验证: 给定相同的 RawEvent 输入, 验证:
/// 1. 脱敏输出是否逐条一致 (PrivacyAirGap 的确定性)
/// 2. 策略引擎的决策是否一致 (PolicyEngine 的确定性)
/// 3. 不一致项生成 divergence report
pub trait TraceValidator {
    fn validate_replay(&self, golden: &GoldenTrace) -> ReplayResult;
}

pub struct ReplayResult {
    pub trace_id: String,
    /// 脱敏输出是否完全一致
    pub sanitization_match: bool,
    /// 不一致的 SanitizedEvent 索引
    pub sanitization_divergences: Vec<usize>,
    /// 策略决策是否完全一致
    pub policy_match: bool,
    /// 不一致的 action 描述
    pub policy_divergences: Vec<String>,
}
```

---

## 六、部署与验证方案

### 6.1 开发阶段 (当前)

```
┌─────────────────────────────────────────────────┐
│ AVD / Genymotion (Android API 34, 模拟器)        │
│                                                 │
│  $ cargo android-release                        │
│  $ adb push target/aarch64-linux-android/       │
│         release/dipecsd /data/local/tmp/        │
│  $ adb shell                                    │
│    su                                           │
│    /data/local/tmp/dipecsd --no-daemon --verbose│
│                                                 │
│  以 root 运行 (模拟器默认 root), 验证:            │
│  - Binder tracepoint 读取                       │
│  - /proc 全量解析                                │
│  - 脱敏输出正确性                                │
│  - Golden Trace 录制与回放                       │
└─────────────────────────────────────────────────┘
```

### 6.2 演示方案

| 方案 | 展示效果 | 准备成本 |
|:---|:---|:---|
| **模拟器 + `adb shell`** | `ps | grep dipecsd` 证明 daemon 在运行, `logcat -s dipecs` 展示结构化事件流 | 低, 现有脚本即可 |
| **模拟器 system image 预置** | daemon 作为 init service 自启, 展示"开机即运行" | 中, 需要打包 system image |
| **真机 (root / custom ROM)** | 真实设备上的端到端演示 | 高, 需要合适的测试机 |
