//! 事件类型定义 —— "what"
//!
//! 从系统采集的原始事件 (`RawEvent`) 到脱敏后的统一事件 (`SanitizedEvent`)。
//! 这是 DiPECS 数据模型的核心。

use serde::{Deserialize, Serialize};

// ============================================================
// RawEvent — 原始事件 (含 PII, 仅存在于 adapter-core 边界内)
// ============================================================

/// 从系统采集的原始事件, 未经脱敏。
///
/// 此类型仅存在于 aios-adapter → aios-core 的传输路径上,
/// 经由 `PrivacySanitizer` 处理后再也不包含原始敏感数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RawEvent {
    /// 应用前后台切换 (UsageStatsManager)
    AppTransition(AppTransitionRawEvent),
    /// Binder 事务 (eBPF tracepoint)
    BinderTransaction(BinderTxEvent),
    /// 进程状态变化 (/proc 轮询)
    ProcStateChange(ProcStateEvent),
    /// 文件系统访问 (fanotify)
    FileSystemAccess(FsAccessEvent),
    /// 通知到达 (NotificationListenerService)
    NotificationPosted(NotificationRawEvent),
    /// 通知交互 (用户点击/清除)
    NotificationInteraction(NotificationInteractionRawEvent),
    /// 屏幕状态变化
    ScreenState(ScreenStateEvent),
    /// 系统状态快照 (周期性采集)
    SystemState(SystemStateEvent),
}

// ===== RawEvent 子类型 =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppTransitionRawEvent {
    pub timestamp_ms: i64,
    pub package_name: String,
    pub activity_class: Option<String>,
    pub transition: AppTransition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppTransition {
    Foreground,
    Background,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinderTxEvent {
    pub timestamp_ms: i64,
    pub source_pid: u32,
    pub source_uid: u32,
    /// e.g. "notification", "activity", "window"
    pub target_service: String,
    /// e.g. "enqueueNotificationWithTag"
    pub target_method: String,
    pub is_oneway: bool,
    /// Parcel 大小, 不存内容
    pub payload_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcStateEvent {
    pub timestamp_ms: i64,
    pub pid: u32,
    pub uid: u32,
    pub package_name: Option<String>,
    pub vm_rss_kb: u64,
    pub vm_swap_kb: u64,
    pub threads: u32,
    /// oom_score: -1000 ~ 1000, 越低越不容易被 LMK 杀死
    pub oom_score: i32,
    pub io_read_mb: u64,
    pub io_write_mb: u64,
    pub state: ProcState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcState {
    Running,
    Sleeping,
    Zombie,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsAccessEvent {
    pub timestamp_ms: i64,
    pub pid: u32,
    pub uid: u32,
    /// 完整文件路径 (脱敏时将只保留扩展名)
    pub file_path: String,
    pub access_type: FsAccessType,
    pub bytes_transferred: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FsAccessType {
    OpenRead,
    OpenWrite,
    Create,
    Delete,
}

/// 通知原始事件 — ⚠️ 含 PII (标题和正文)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRawEvent {
    pub timestamp_ms: i64,
    pub package_name: String,
    pub category: Option<String>,
    pub channel_id: Option<String>,
    /// ⚠️ PII
    pub raw_title: String,
    /// ⚠️ PII
    pub raw_text: String,
    pub is_ongoing: bool,
    pub group_key: Option<String>,
    pub has_picture: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationInteractionRawEvent {
    pub timestamp_ms: i64,
    pub package_name: String,
    pub notification_key: String,
    pub action: NotificationAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationAction {
    Tapped,
    Dismissed,
    Cancelled,
    Seen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenStateEvent {
    pub timestamp_ms: i64,
    pub state: ScreenState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStateEvent {
    pub timestamp_ms: i64,
    pub battery_pct: Option<u8>,
    pub is_charging: bool,
    pub network: NetworkType,
    pub ringer_mode: RingerMode,
    pub location_type: LocationType,
    pub headphone_connected: bool,
    pub bluetooth_connected: bool,
}

// ============================================================
// SanitizedEvent — 脱敏后事件 (不含 PII, 可在系统内自由传输)
// ============================================================

/// 脱敏后的事件。不再包含任何 PII。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedEvent {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub event_type: SanitizedEventType,
    pub source_tier: SourceTier,
    pub app_package: Option<String>,
    pub uid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SanitizedEventType {
    /// 应用前后台切换 (来自 UsageStatsManager)
    AppTransition {
        package_name: String,
        activity_class: Option<String>,
        transition: AppTransition,
    },
    /// 应用间交互 (从 Binder 事务推断)
    InterAppInteraction {
        source_package: Option<String>,
        target_service: String,
        interaction_type: InteractionType,
    },
    /// 通知 (脱敏后)
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
        is_hot_file: bool,
    },
    /// 屏幕状态
    Screen { state: ScreenState },
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

// ============================================================
// 辅助枚举 — 跨类型共用
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceTier {
    PublicApi = 0,
    Daemon = 1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InteractionType {
    NotifyPost,
    ActivityLaunch,
    ShareIntent,
    ServiceBind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextHint {
    pub length_chars: usize,
    pub script: ScriptHint,
    pub is_emoji_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptHint {
    Latin,
    Hanzi,
    Cyrillic,
    Arabic,
    Mixed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticHint {
    FileMention,
    ImageMention,
    AudioMessage,
    LinkAttachment,
    UserMentioned,
    CalendarInvitation,
    FinancialContext,
    VerificationCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExtensionCategory {
    Document,
    Image,
    Video,
    Audio,
    Archive,
    Code,
    Other,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FsActivityType {
    Read,
    Write,
    Create,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScreenState {
    Interactive,
    NonInteractive,
    KeyguardShown,
    KeyguardHidden,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkType {
    Wifi,
    Cellular,
    Offline,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RingerMode {
    Normal,
    Vibrate,
    Silent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LocationType {
    Home,
    Work,
    Commute,
    Unknown,
}
