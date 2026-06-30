# Android 接口最小可运行边界

> Status: Partially current  
> Last verified: 2026-06-30  
> 当前生产入口已经收敛为 append-only JSONL；JNI / local socket ingress 只作为历史讨论或后续替换路线。

## Current Production Decision

`apps/android-collector` is no longer treated only as a Phase-1 screening app.
It remains the Android-side collector for public API signals, while Rust owns
the production interpretation boundary.

Promoted production sources:

- `UsageStatsManager` -> `RawEvent::AppTransition`
- `NotificationListenerService` -> `RawEvent::NotificationPosted` /
  `RawEvent::NotificationInteraction`
- `DeviceContext` -> `RawEvent::SystemState`

These sources write append-only JSONL rows with a non-null `rawEvent`.
`dipecsd --android-trace-jsonl <actions.jsonl>` tails that file, wraps each row
as `CollectorEnvelope { source_tier: PublicApi, ... }`, and sends it through
`PrivacyAirGap -> WindowAggregator -> DecisionRouter -> PolicyEngine`.

Sources without an accepted Rust schema, currently including
`AccessibilityService`, stay in screening mode. Their rows may remain useful in
the app preview, but `rawEvent: null` rows are skipped by production Rust
ingress.

> 日期: 2026-05-05  
> 范围: 梳理 `apps/android-collector` 这类 Android 采集能力如何接入 `aios-collector`, 再进入现有 Rust 管道。

## 目标

当前最小目标不是把 Android App 写完, 而是先固定一条可运行、可测试的接口边界:

```text
Android API / Kotlin service
    -> apps/android-collector (采集能力来源)
    -> aios-collector (append-only JSONL ingress)
    -> CollectorEnvelope / RawEvent
    -> aios-core (PrivacyAirGap -> WindowAggregator)
    -> StructuredContext
    -> DecisionRouter
    -> PolicyEngine
    -> ActionExecutor
```

Rust 侧的后半段已经在 v0.2 中打通。现在最需要稳定的是 Android 公开 API 到 `aios-collector` 的入口约定: app 侧负责拿到真实观测, collector 负责规范化为 `CollectorEnvelope` / `RawEvent`, core 再负责脱敏和 `StructuredContext`。

## 最小可用数据源

| Android 接口 | 权限/前提 | 可观测事实 | Rust 入口 | MVP 状态 |
| --- | --- | --- | --- | --- |
| `NotificationListenerService` | 用户授权通知访问 | 通知来源包名、类别、渠道、标题/正文 extras、发布时间、通知移除原因 | `RawEvent::NotificationPosted`, `RawEvent::NotificationInteraction` | 可直接对接 |
| `BatteryManager` | 无需敏感权限 | 电量、充电状态 | `RawEvent::SystemState` | 可直接对接 |
| `ConnectivityManager` | `ACCESS_NETWORK_STATE` | Wi-Fi/蜂窝/离线、是否按流量计费 | `RawEvent::SystemState` | 可直接对接 |
| `AudioManager` | 无需敏感权限 | 铃声/震动/静音模式 | `RawEvent::SystemState` | 可直接对接 |
| `PowerManager` / 屏幕广播 | 普通系统能力 | 亮屏、灭屏、锁屏显示/隐藏 | `RawEvent::ScreenState` | 可直接对接 |
| `UsageStatsManager.queryEvents()` | `PACKAGE_USAGE_STATS` 用户授权 | `ACTIVITY_RESUMED` / `ACTIVITY_PAUSED`、解锁、屏幕状态等事件 | `RawEvent::AppTransition` | 已补 spec, 可对接 |
| `MediaStore` / `ContentObserver` | 媒体/文件访问权限按版本变化 | 公开媒体或下载目录变化 | 当前 `RawEvent::FileSystemAccess` 偏 daemon 文件路径模型 | 可后置 |
| `AccessibilityService` | 用户显式授权, 审查和性能成本高 | UI 控件树、窗口切换、点击/滑动 | 需要新增可选 Tier 1 事件 | 不进 MVP |

MVP 只应依赖 Tier 0 公开接口。`AccessibilityService` 可以作为增强层, 但不应该成为可运行闭环的前提。Binder eBPF、fanotify、root/Shizuku 路线也不应作为当前 Android 接口 MVP 的依赖, 因为它们要么权限成本过高, 要么无法提供应用层语义。

## 当前 Rust 侧可直接接收的事件

现有 `aios-spec` 已经能表达四类 Android 公开 API 事件:

```json
{
  "NotificationPosted": {
    "timestamp_ms": 1714789201000,
    "package_name": "com.ss.android.lark",
    "category": "msg",
    "channel_id": "lark_im_message",
    "raw_title": "张三",
    "raw_text": "发来一个文件: report.pdf",
    "is_ongoing": false,
    "group_key": "lark_conversation_xxx",
    "has_picture": false
  }
}
```

```json
{
  "NotificationInteraction": {
    "timestamp_ms": 1714789210000,
    "package_name": "com.ss.android.lark",
    "notification_key": "0|com.ss.android.lark|42|null|10086",
    "action": "Tapped"
  }
}
```

```json
{
  "ScreenState": {
    "timestamp_ms": 1714789220000,
    "state": "Interactive"
  }
}
```

```json
{
  "SystemState": {
    "timestamp_ms": 1714789230000,
    "battery_pct": 78,
    "is_charging": false,
    "network": "Wifi",
    "ringer_mode": "Normal",
    "location_type": "Unknown",
    "headphone_connected": false,
    "bluetooth_connected": false
  }
}
```

这些 JSON 使用 Rust `serde` 对枚举的默认外部标签格式。当前生产入口是 JSONL tail；如果未来替换为 JNI 或 socket，也应保持这一 schema，避免 Android 层和 Rust 层各自定义一套格式。

Android 回调到 Rust 事件的最小映射如下:

- `NotificationListenerService.onNotificationPosted(...)` 生成 `NotificationPosted`。
- `NotificationListenerService.onNotificationRemoved(..., reason)` 生成 `NotificationInteraction`; `REASON_CLICK` 映射为 `Tapped`, 用户清除类 reason 映射为 `Dismissed`, 应用主动取消映射为 `Cancelled`。
- `UsageEvents.Event.ACTIVITY_RESUMED` 映射为 `AppTransition::Foreground`, `ACTIVITY_PAUSED` 映射为 `AppTransition::Background`。旧的 `MOVE_TO_FOREGROUND` / `MOVE_TO_BACKGROUND` 已在 API 29 被弃用, 只作为兼容兜底。
- `ConnectivityManager.getNetworkCapabilities(...)` 只取网络 transport / metered 这类粗粒度字段, 不读取带位置敏感含义的 Wi-Fi 细节。

## 已补齐的关键接口

`UsageStatsManager` 是 Android 侧最关键的行为接口。当前 `RawEvent::AppTransition` 已经补齐, 可以作为公开 API 级的 App 前后台事件进入 collector/core 管道。这样 `ContextSummary.foreground_apps` 不再只能从 `/proc` 或 `InterAppInteraction` 间接聚合。

当前事件形态:

```rust
pub enum RawEvent {
    AppTransition(AppTransitionRawEvent),
    // ...
}

pub struct AppTransitionRawEvent {
    pub timestamp_ms: i64,
    pub package_name: String,
    pub activity_class: Option<String>,
    pub transition: AppTransition,
}

pub enum AppTransition {
    Foreground,
    Background,
}
```

脱敏后进入 `SanitizedEventType::AppTransition`。这样 `UsageStatsManager.queryEvents()` 能稳定进入窗口聚合, `ContextSummary.foreground_apps` 也就有了公开 API 来源。

## 最小演示闭环

第一版可运行演示只需要三类输入:

1. `UsageStatsManager` 产生 App 前后台切换事件。
2. `NotificationListenerService` 产生通知到达和通知交互事件。
3. 系统服务产生屏幕、电量、网络、铃声等状态快照。

这三类事件进入 Rust 后, 现有管道已经可以完成:

- 通知正文在 `PrivacyAirGap` 内转成 `TextHint` 和 `SemanticHint`, 原文不越过脱敏边界。
- `WindowAggregator` 按 10 秒窗口聚合上下文。
- `DecisionRouter` 当前可通过 `RuleBasedBackend` 根据 `FileMention`、前台切换、屏幕状态、系统状态生成低风险意图；后续可接 LocalEvaluator / CloudLlm。
- `PolicyEngine` 和 `ActionExecutor` 能记录低风险动作结果。

因此当前写作结论是: Android MVP 的接口边界应优先把 `apps/android-collector` 观测到的 `UsageStatsManager -> AppTransitionRawEvent` 和 `NotificationListenerService -> NotificationRawEvent` 接入 `aios-collector`; eBPF、fanotify 和 system image 路线作为后续 system 下沉能力增强。

## 采集正确性的观测方式

采集行为不能只靠“代码跑了”来判断, 需要在三个层次留下证据:

1. 原始入口层: daemon 在每个窗口关闭时输出 `raw_event_total` 和 `raw_event_stats`, 例如 `app_transition=3 notification_posted=1 system_state=1`。如果手动切换 App 后没有看到 `app_transition`, 说明 Android 入口或桥接没有把事件送进 Rust。
2. 脱敏边界层: `PrivacyAirGap` 测试验证 `RawEvent::AppTransition` 会变成 `SanitizedEventType::AppTransition`, 通知正文会变成 `TextHint` / `SemanticHint`, 原文不会越过边界。
3. 窗口语义层: `WindowAggregator` 测试验证 `AppTransition::Foreground` 会进入 `ContextSummary.foreground_apps`; `DecisionRouter` / `RuleBasedBackend` 测试验证这个前台切换能触发 `SwitchToApp` 意图。

这三层分别回答: “采到了没有”、“脱敏后是否还保留正确语义”、“后续推理是否能用到这个行为”。调试 Android 入口时优先看第一层日志, 回归测试时优先跑后两层测试。

Android collector 侧也保留同一套观测口径: JSONL 事件中新增 `rawEvent` 字段, 使用 Rust `RawEvent` 的 serde 外部标签格式。例如 UsageStats 前台事件会写成 `{"AppTransition": {...}}`, 通知到达会写成 `{"NotificationPosted": {...}}`, 设备状态 heartbeat 会写成 `{"SystemState": {...}}`。App 内 trace preview 会显示 `raw=<kind>`, 便于在真机上先确认采集源是否产出了 Rust 可消费事件。

## 接口核对依据

- Android Developers: [`UsageEvents.Event`](https://developer.android.com/reference/android/app/usage/UsageEvents.Event) (`ACTIVITY_RESUMED`, `ACTIVITY_PAUSED`, deprecated `MOVE_TO_*`)
- Android Developers: [`NotificationListenerService`](https://developer.android.com/reference/android/service/notification/NotificationListenerService) (`onNotificationPosted`, `onNotificationRemoved`, `REASON_CLICK`)
- Android Developers: [`Notification`](https://developer.android.com/reference/android/app/Notification) extras (`EXTRA_TITLE`, `EXTRA_TEXT`, `EXTRA_BIG_TEXT`)
- Android Developers: [`ConnectivityManager.getNetworkCapabilities`](https://developer.android.com/reference/android/net/ConnectivityManager#getNetworkCapabilities(android.net.Network))
- Android Developers: [`BatteryManager`](https://developer.android.com/reference/android/os/BatteryManager) / `ACTION_BATTERY_CHANGED`
