# Android 数据源能力调研与结构化观测设想

> **日期**: 2026-05-04
> **目的**: 为 `aios-spec` 阶段的 `Observation` 数据结构设计提供事实基础
> **核心问题**: Android 公开 API 能拿到什么信息？哪些信息拿不到？需要 eBPF 等手段吗？

---

## 1. 可用数据源全景 (按能力等级分层)

### 1.1 Tier 0 — 基础采集 (公开 API, 仅需用户授权)

| 数据源 | 权限 | 可获取信息 | API 层级限制 |
|:---|:---|:---|:---|
| **UsageStatsManager** | `PACKAGE_USAGE_STATS` (用户可授权) | 应用前后台切换 (精确到 ms)、使用时长、屏幕交互/非交互状态、锁屏显示/隐藏、通知被查看事件 (`NOTIFICATION_SEEN`)、快捷方式调用、前台服务启停、设备启动/关机、设备解锁 | 事件仅保留数天 |
| **NotificationListenerService** | `BIND_NOTIFICATION_LISTENER_SERVICE` (用户可授权) | 通知标题 (`EXTRA_TITLE`)、正文 (`EXTRA_TEXT`)、展开文本 (`EXTRA_BIG_TEXT`)、副标题 (`EXTRA_SUB_TEXT`)、摘要 (`EXTRA_SUMMARY_TEXT`)、信息文本 (`EXTRA_INFO_TEXT`)、通知类别 (`category`)、渠道 ID (`channelId`)、来源包名 (`packageName`)、发布时间戳 (`postTime`)、分组 Key (`groupKey`)、是否常驻 (`isOngoing`)、是否可清除 (`isClearable`)、进度条、大图标 | Android 15 起对 OTP 类敏感通知自动拦截；部分 Google 应用故意不填 extras |
| **BatteryManager** | 无需权限 | 电量百分比、充电状态 (AC/USB/无线)、低电量状态 | — |
| **ConnectivityManager** | `ACCESS_NETWORK_STATE` | 网络类型 (WiFi/蜂窝/离线)、子类型 (4G/5G)、是否按流量计费 | — |
| **AudioManager** | 无需权限 | 铃声模式 (正常/振动/静音)、音量 | — |
| **PowerManager** | 无需权限 | 屏幕是否亮起 (`isInteractive`) | — |
| **LocationManager** | `ACCESS_COARSE_LOCATION` (用户授权) | 粗粒度位置 (可用于推断地点类别: 家/公司/通勤) | Android 10+ 仅前台可获取精确位置 |

### 1.2 Tier 1 — 增强采集 (公开 API, 但安装/权限门槛高)

| 数据源 | 权限 | 可获取信息 | 限制与风险 |
|:---|:---|:---|:---|
| **AccessibilityService** | `BIND_ACCESSIBILITY_SERVICE` (用户授权) | 完整 UI 控件树 (文本内容、类名、资源 ID、contentDescription、是否可点击/聚焦)、窗口切换事件、用户点击/滑动事件、WebView 内容 (需要 `flagRequestEnhancedWebAccessibility`)、键盘输入 (需 `flagRequestFilterKeyEvents`) | Android 13+ 侧载应用默认被阻止启用；Android 14 进一步收紧；Android 15 "增强确认模式"基本终结非无障碍用途；Google Play 严格审查无障碍服务使用；显著增加电量和性能开销 |

### 1.3 Tier 2 — 开发者级 (需要 ADB / Shizuku)

| 数据源 | 前提条件 | 可获取信息 | 限制 |
|:---|:---|:---|:---|
| **Shizuku (app_process + Binder)** | 开发者选项开启 + 无线调试 (Android 11+) 或 USB ADB | 通过 `dumpsys` 获取完整系统状态 (进程列表、Service 状态、内存详情、AppOps 全量)、`logcat` 全量日志、访问受保护的 `Android/data` 目录、禁用/冻结应用组件、管理权限 | 每次重启后需重新激活；需要开发者模式；部分银行应用会检测并拒绝运行 |

### 1.4 Tier 3 — 内核级 (需要 Root 或 自编译 ROM)

| 手段 | 前提条件 | 理论能力 | 实际结论 |
|:---|:---|:---|:---|
| **eBPF** | Root 或自编译 ROM (需修改 bionic seccomp + SELinux policy + kernel sysctl) | 内核级系统调用追踪、网络包过滤、进程调度观测 | **不可行**。即使解锁三层限制，非特权 eBPF 只能加载 `BPF_PROG_TYPE_SOCKET_FILTER`，无法做 tracing/kprobe。对于理解应用层语义没有帮助。 |
| **LSPosed / Xposed** | Root + Magisk | Hook 任意 Java 方法、拦截 Intent、读取任意 App 内部状态 | 可行但不属于项目范围 (依赖 Root，违反"仅使用公开 API"约束) |

---

## 2. 关键场景分析: 飞书收到文件

### 2.1 实际能观测到的信息 (Tier 0)

当联系人通过飞书发送文件时，本地能通过 `NotificationListenerService` 获取:

| 字段 | 示例值 | 说明 |
|:---|:---|:---|
| `packageName` | `com.ss.android.lark` | 飞书包名 |
| `category` | `"msg"` (`CATEGORY_MESSAGE`) | **注意: Android 不存在 `CATEGORY_FILE` 常量**。飞书大概率将文件消息归类为普通消息通知 |
| `EXTRA_TITLE` | `"张三"` 或 `"飞书"` | 通知标题 (通常为发送者名称) |
| `EXTRA_TEXT` | `"张三发来一个文件: report.pdf"` 或 `"[文件]"` | 正文可能包含文件名、也可能只是通用描述，完全取决于飞书 App 的实现 |
| `EXTRA_BIG_TEXT` | 可能包含更多消息预览 | 展开通知时的完整内容 |
| `EXTRA_SUMMARY_TEXT` | 可能为消息摘要 | 部分应用填充此字段 |
| `channelId` | 飞书定义的通知渠道 | 可用于区分消息类型 (如果飞书用不同渠道区分) |
| `postTime` | `1714789200000` | 通知发布时间戳 |

### 2.2 拿不到的信息

| 缺失信息 | 原因 |
|:---|:---|
| 文件是否为"文件类型"消息 (vs 普通文本) | 通知 API 不区分消息语义；`CATEGORY_FILE` 不存在 |
| 文件名 / 文件大小 / MIME 类型 | 除非飞书主动写入 notification extras，否则无法获取 |
| 文件下载状态 / 是否已打开 | 应用内部状态，对外不可见 |
| 文件存储路径 | 若飞书未通过 MediaStore 或公开目录存储文件，`ContentObserver` 也观测不到 |

### 2.3 可提升的观测精度

| 手段 | 代价 | 效果 |
|:---|:---|:---|
| **文本启发式判断** (对 `EXTRA_TEXT` 关键词匹配) | 零额外代价 | 可识别"文件""pdf""docx"等关键词，准确率取决于应用通知格式，不可靠 |
| **AccessibilityService** (Tier 1) | 高权限门槛 + 电量开销 + Play 审查风险 | 可读取聊天界面的 UI 控件树，从中提取文件消息的具体内容 (文件名、文件类型标签等) |
| **Shizuku + dumpsys** (Tier 2) | 需要开发者模式 | 可获取飞书进程的内存/网络使用情况，可间接判断是否有大量数据传输，但无法获取文件语义 |
| **ContentObserver** (Tier 0) | 仅在文件写入公开目录时有效 | 若飞书将接收的文件保存到 `Download/` 或 `Documents/` 目录，可检测到新文件创建 |
| **eBPF / Root** (Tier 3) | 完全不可行 | eBPF 只能观测 syscall 层面，无法还原应用层语义；Root 可行但违反项目约束 |

### 2.4 结论: eBPF 不是答案

对于"飞书收文件"这个场景，**eBPF 无法解决问题**:

- 即使有 root 部署 eBPF，也只能看到 `open()`、`write()`、`socket()` 等系统调用
- 文件语义 (文件名、类型、发件人) 存在于应用层数据结构中，不是 eBPF 的观测目标
- 正确的解决方向是: **(a)** 接受 Tier 0 的信息不完整性，依赖 LLM 从上下文推断; **(b)** 使用 Tier 1 (AccessibilityService) 作为可选增强; **(c)** 将不确定性编码到数据结构中

---

## 3. 结构化观测模型设计

### 3.1 设计原则

1. **能力分层, 诚实表达不确定性**: 每个观测事件标注 `source_tier`, LLM 据此决定推断的激进程度
2. **最小可观测单元**: 一个事件只描述一项可观测事实, 不做提前聚合
3. **语义字段独立于采集源**: 同一个字段可通过不同 tier 的数据源填充, 但有一个规范的语义定义
4. **缺失字段不填默认值**: 使用 `Option<T>`, `None` 就表示"没观测到", 而不是"不存在"
5. **隐私边界前置**: 任何可能含 PII 的字段 (通知正文、UI 文本) 在进入 Core 层前必须脱敏

### 3.2 观测事件统一 Schema

```rust
/// 观测时间窗口内的一组事件
pub struct ObservationWindow {
    /// 窗口起始时间 (epoch ms)
    pub window_start_ms: i64,
    /// 窗口结束时间 (epoch ms)
    pub window_end_ms: i64,
    /// 窗口内的事件列表
    pub events: Vec<ObservationEvent>,
}

/// 单一观测事件
pub struct ObservationEvent {
    /// 事件唯一 ID
    pub event_id: String,
    /// 事件发生时间戳 (epoch ms)
    pub timestamp_ms: i64,
    /// 事件类型
    pub event_type: EventType,
    /// 数据来源能力等级
    pub source_tier: SourceTier,
    /// 关联的 app (若可确定)
    pub app_package: Option<String>,
}

/// 数据来源能力等级
pub enum SourceTier {
    /// Tier 0: 公开 API, 普通用户授权即可
    Basic = 0,
    /// Tier 1: 无障碍服务 (权限门槛高, 电量开销大)
    Accessibility = 1,
    /// Tier 2: ADB / Shizuku (需要开发者模式)
    Developer = 2,
    /// Tier 3: Root (不在本项目范围内)
    Root = 3,
}

/// 事件类型枚举
pub enum EventType {
    AppTransition(AppTransitionEvent),
    NotificationPosted(NotificationEvent),
    NotificationInteraction(NotificationInteractionEvent),
    ScreenState(ScreenStateEvent),
    SystemState(SystemStateEvent),
    UserInteraction(UserInteractionEvent),
    WindowContent(WindowContentEvent),
    FileSystemChange(FileSystemEvent),
}

// ===== 具体事件结构 =====

/// 应用前后台切换
pub struct AppTransitionEvent {
    /// MOVE_TO_FOREGROUND / MOVE_TO_BACKGROUND
    pub transition: AppTransition,
    /// 触发此事件的 Activity 类名 (可能为空)
    pub activity_class: Option<String>,
}

/// 通知到达
pub struct NotificationEvent {
    /// 通知来源 app
    pub source_package: String,
    /// 通知类别 (Android category 原始值, 如 "msg", "email", "social")
    pub category: Option<String>,
    /// 通知渠道 ID
    pub channel_id: Option<String>,
    /// 脱敏后的通知标题 (仅保留长度、语言类别, 不保留原文)
    pub title_hint: Option<TextHint>,
    /// 脱敏后的通知正文 (仅保留长度、语言类别, 不保留原文)
    pub text_hint: Option<TextHint>,
    /// 从正文中提取的脱敏关键词 (如 "文件", "图片", "语音", "链接")
    pub semantic_hints: Vec<SemanticHint>,
    /// 通知是否持续显示 (ongoing)
    pub is_ongoing: bool,
    /// 通知分组 key
    pub group_key: Option<String>,
    /// 通知是否包含进度条
    pub has_progress: bool,
    /// 通知是否包含图片
    pub has_picture: bool,
}

/// 脱敏后的文本提示 (不传输原文)
pub struct TextHint {
    /// 文本长度
    pub length_chars: usize,
    /// 主要书写系统 (Latin, Hanzi, Cyrillic, Arabic, etc.)
    pub script: ScriptHint,
    /// 是否为纯 emoji
    pub is_emoji_only: bool,
}

/// 从通知文本中提取的语义标签 (在本地完成, 不上传原文)
pub enum SemanticHint {
    /// "文件" / "file" / "pdf" / "docx" 等关键词
    FileMention,
    /// "图片" / "照片" / "截图" 等关键词
    ImageMention,
    /// 音频/语音消息
    AudioMessage,
    /// 链接/URL
    LinkAttachment,
    /// "@我" / 被提及
    UserMentioned,
    /// 会议/日历邀请
    CalendarInvitation,
}

/// 用户与通知的交互
pub struct NotificationInteractionEvent {
    pub notification_key: String,
    pub action: NotificationAction,
}

pub enum NotificationAction {
    /// 用户点击通知
    Tapped,
    /// 用户清除通知
    Dismissed,
    /// 系统自动取消
    Cancelled,
    /// 通知已被查看 (NOTIFICATION_SEEN)
    Seen,
}

/// 屏幕状态变化
pub struct ScreenStateEvent {
    pub state: ScreenState,
}

pub enum ScreenState {
    Interactive,     // 亮屏 + 解锁
    NonInteractive,  // 灭屏
    KeyguardShown,   // 锁屏显示
    KeyguardHidden,  // 锁屏隐藏
    UserUnlocked,    // 用户解锁设备
}

/// 系统状态快照 (周期性采集, 非事件驱动)
pub struct SystemStateEvent {
    /// 电量百分比 (0-100)
    pub battery_pct: Option<u8>,
    /// 是否充电中
    pub is_charging: bool,
    /// 网络类型
    pub network: NetworkType,
    /// 铃声模式
    pub ringer_mode: RingerMode,
    /// 粗粒度地点类别
    pub location_type: LocationType,
    /// 是否连接耳机
    pub headphone_connected: bool,
    /// 是否连接蓝牙
    pub bluetooth_connected: bool,
}

/// 用户交互事件 (仅 Tier 1: AccessibilityService)
pub struct UserInteractionEvent {
    /// 点击/长按/滑动
    pub action: InteractionAction,
    /// 被交互的 UI 元素的脱敏描述
    pub target_hint: Option<TargetHint>,
}

/// 窗口内容摘要 (仅 Tier 1: AccessibilityService)
/// 不上传完整 UI 树, 仅上传聚合后的语义描述
pub struct WindowContentEvent {
    /// 当前前台 app
    pub package_name: String,
    /// 窗口标题 (脱敏)
    pub window_title_hint: Option<TextHint>,
    /// 当前 UI 的语义类型推断
    pub ui_type_hint: Option<UiTypeHint>,
    /// UI 中出现的脱敏语义标签列表
    pub semantic_labels: Vec<SemanticHint>,
}

/// 文件系统变化 (Tier 0: ContentObserver)
pub struct FileSystemEvent {
    /// 变化的目录或 URI
    pub path_pattern: String,
    /// 文件扩展名 (如 "pdf", "docx")
    pub extension: Option<String>,
    /// 变化类型
    pub change_type: FileChangeType,
}

// ===== 辅助枚举 =====

pub enum AppTransition {
    Foreground,
    Background,
}

pub enum NetworkType {
    Wifi,
    Cellular,
    Offline,
    Unknown,
}

pub enum RingerMode {
    Normal,
    Vibrate,
    Silent,
}

pub enum LocationType {
    Home,
    Work,
    Commute,
    Unknown,
}

pub enum ScriptHint {
    Latin,
    Hanzi,      // 汉字
    Cyrillic,
    Arabic,
    Mixed,
    Unknown,
}

pub enum UiTypeHint {
    ChatList,
    ChatDetail,
    FilePreview,
    Settings,
    WebView,
    VideoCall,
    Unknown,
}

pub enum InteractionAction {
    Tap,
    LongPress,
    Swipe,
    Scroll,
    KeyPress,
}

pub enum TargetHint {
    Button,
    TextField,
    Image,
    Link,
    ListItem,
    Unknown,
}

pub enum FileChangeType {
    Created,
    Modified,
    Deleted,
}
```

### 3.3 飞书收文件场景的结构化表示 (Tier 0)

基于以上 schema, 飞书收到文件时本地将产生如下结构化观测:

```json
{
  "window_start_ms": 1714789200000,
  "window_end_ms": 1714789260000,
  "events": [
    {
      "event_id": "evt_001",
      "timestamp_ms": 1714789201000,
      "event_type": {
        "NotificationPosted": {
          "source_package": "com.ss.android.lark",
          "category": "msg",
          "channel_id": "lark_im_message",
          "title_hint": {
            "length_chars": 2,
            "script": "Hanzi",
            "is_emoji_only": false
          },
          "text_hint": {
            "length_chars": 18,
            "script": "Mixed",
            "is_emoji_only": false
          },
          "semantic_hints": [
            "FileMention"
          ],
          "is_ongoing": false,
          "group_key": "lark_conversation_xxx",
          "has_progress": false,
          "has_picture": false
        }
      },
      "source_tier": "Basic",
      "app_package": "com.ss.android.lark"
    }
  ]
}
```

**关键点**:

- `semantic_hints: ["FileMention"]` 来自本地关键词匹配 (检测到正文含"文件"), 而非 Android API 直接提供
- LLM 知道这是 Tier 0 数据 (`source_tier: "Basic"`), 因此可以理解文件类型的判断可能不准确
- 具体的文件名、大小、下载状态等字段全部为 `None`, 诚实地表达"没观测到"

### 3.4 同一场景在 Tier 1 (AccessibilityService 启用) 的增强表示

```json
{
  "WindowContent": {
    "package_name": "com.ss.android.lark",
    "window_title_hint": {
      "length_chars": 3,
      "script": "Hanzi",
      "is_emoji_only": false
    },
    "ui_type_hint": "ChatDetail",
    "semantic_labels": [
      "FileMention",
      "ImageMention"
    ]
  }
}
```

Tier 1 额外提供了 `ui_type_hint: "ChatDetail"` (确认用户在聊天详情页) 和更丰富的 `semantic_labels` (除了文件还有图片), 使 LLM 的场景判断更准确。

---

## 4. 关键发现与建议

### 4.1 eBPF 结论: 不适用于本项目

| 阻碍 | 详情 |
|:---|:---|
| **seccomp-bpf 过滤** | zygote 对 app 进程屏蔽 `bpf()` 系统调用 |
| **SELinux 强制访问控制** | bpf 权限仅授予 `bpfloader`/`netd`/`MediaProvider` 等系统守护进程, 拒绝 `untrusted_app` |
| **内核级禁用** | `kernel.unprivileged_bpf_disabled=2` (Linux 5.19+) — 非特权进程永久禁止加载 eBPF 程序 |
| **能力范围有限** | 即使解锁所有限制, 非特权 eBPF 也只能加载 `SOCKET_FILTER` 类型, 无法做 tracing、kprobe 等内核观测 |
| **语义鸿沟** | 即使能做 tracing, eBPF 观测的是系统调用层面 (文件描述符、内存页), 不是应用语义层面 (文件消息 vs 文本消息) |

**结论**: 对于"获取飞书收到文件"这类应用层语义需求, eBPF 完全不是正确的工具。

### 4.2 Shizuku (Tier 2) 的价值评估

Shizuku 可以提供 `dumpsys` 级别的信息 (如进程内存、Service 状态), 但对于应用层语义理解帮助有限。它更适合用于**调试和可观测性基础设施** (如获取更细粒度的系统状态), 而不是行为语义采集。

### 4.3 信息缺口与应对策略

| 缺口 | 应对策略 |
|:---|:---|
| 通知内容不含文件元数据 | **文本启发式** (关键词匹配 文件/图片/语音/链接), 将不确定性编码为 `semantic_hints` |
| 应用内部 UI 不可见 | **可选启用 AccessibilityService** (Tier 1), 将 `ui_type_hint` 传给 LLM |
| 无法区分同 App 的不同通知来源 (群聊 vs 私聊) | 利用 `channelId` 和 `groupKey` 进行推断 |
| 某些 App 的 notification extras 为空 | 编码为 `title_hint: None, text_hint: None` — LLM 收到缺失信息后可以做保守推断 |
| 跨 App 的用户操作链无法完整追踪 | 通过 `UsageStatsManager.queryEvents()` 获取前后台切换时间线, 配合通知时间戳拼接链路 |

### 4.4 对 `aios-spec` 设计的建议

1. **`Observation` 结构体必须包含 `source_tier` 字段** — 这是 DiPECS 架构的独特信息, 让云端 LLM 知道它能信任多少
2. **`NotificationEvent` 不包含原始文本** — 只包含脱敏后的 `TextHint` 和本地提取的 `SemanticHint`, 在 `aios-core` 完成脱敏
3. **`WindowContentEvent` 设计为可插拔的** — Tier 0 下为空, Tier 1 下有内容, 不需要在 Spec 层强制
4. **预留 eBPF 接口但标注为实验性/不可用** — 如果未来 Android 开放 eBPF, 可以通过新增 `source_tier` 变体接入
5. **`SystemStateEvent` 作为周期性快照而非事件驱动** — 通过窗口聚合, 减少网络请求频率
