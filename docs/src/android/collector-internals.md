# Android 采集器内部实现

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `apps/android-collector/app/src/main/java/com/dipecs/collector/`

**这篇文档回答什么**：Android app 内部如何采集事件、存储 trace、启动 action socket 以及执行动作。  
**适合谁读**：需要理解采集链路、排查 Android 端行为或扩展新数据源的人。

## TL;DR

Android 采集器是一条单应用内的管线：

```text
Foreground Service (Usage / Notification / DeviceContext)
  -> EventRepository
  -> EventStore (append-only JSONL)
  -> AuthorizedActionSocketServer (localhost)
  -> ActionExecutorBridge
  -> SystemActionExecutors / CacheTrimmer / Prefetcher
```

所有落盘数据都经过 `EventStore.sanitizeForTrace`；原始通知文本等敏感字段在写入前被清空。

## Foreground Service

`CollectorForegroundService` 是采集主入口，启动后做三件事：

| Runnable | 周期 | 职责 |
| --- | --- | --- |
| `pollRunnable` | 5 s | 调用 `UsageCollector.collectSinceLastPoll()` |
| `heartbeatRunnable` | 30 s | 采集并记录 `device_context` heartbeat |
| `uploadRunnable` | 60 s | 周期性上传最近 100 条 sanitized events（需手动开启） |

启动命令：

```kotlin
val intent = Intent(context, CollectorForegroundService::class.java)
intent.action = CollectorForegroundService.ACTION_START
ContextCompat.startForegroundService(context, intent)
```

`onCreate()` 会初始化 `UsageCollector`、通知渠道，并启动 `AuthorizedActionSocketServer`。

## BootReceiver

`BootReceiver` 在收到 `BOOT_COMPLETED` 后尝试自启 Foreground Service。

注意：普通 App 安装的 collector **不会**收到 `BOOT_COMPLETED`；只有 system/priv-app 才会生效。

## 事件采集源

### UsageCollector

基于 `UsageStatsManager.queryEvents()`，轮询间隔 5 秒。

| Usage Event | RawEvent |
| --- | --- |
| `ACTIVITY_RESUMED` / `MOVE_TO_FOREGROUND` | `AppTransition::Foreground` |
| `ACTIVITY_PAUSED` / `MOVE_TO_BACKGROUND` / `ACTIVITY_STOPPED` | `AppTransition::Background` |
| `SCREEN_INTERACTIVE` | `ScreenState::Interactive` |
| `SCREEN_NON_INTERACTIVE` | `ScreenState::NonInteractive` |
| `KEYGUARD_SHOWN` | `ScreenState::KeyguardShown` |
| `KEYGUARD_HIDDEN` | `ScreenState::KeyguardHidden` |

### NotificationCollectorService

基于 `NotificationListenerService`：

- `onNotificationPosted` → `RawEvent::NotificationPosted`
- `onNotificationRemoved(reason)` → `RawEvent::NotificationInteraction`
  - `REASON_CLICK` → `Tapped`
  - 用户清除类 reason → `Dismissed`
  - 其他 → `Cancelled`

标题和正文仅在本地用于提取 `TextHint` / `SemanticHint`，落盘前被清空。

### DeviceContextCollector

每次 heartbeat 采集：

- 电量、充电状态
- 网络类型（Wi-Fi / 蜂窝 / 离线等）
- 屏幕是否亮
- 铃声模式
- 勿扰模式
- 耳机/蓝牙连接状态
- 时区

## EventStore 与 Trace

本地 trace 路径：

```text
<app-private-files>/traces/actions.jsonl
```

导出路径：

```text
/sdcard/Android/data/com.dipecs.collector/files/traces/actions.jsonl
```

`sanitizeForTrace` 会覆盖以下敏感字段：

- 置为 `JSONObject.NULL`：`group_key`、`key`、`tag`、`payload`、`responseBody`、
  `sourceText`、`sourceContentDescription`、`textItems`、`windowTitle`、`text`、
  `target`、`cachePath`
- 置为空字符串：`raw_title`、`raw_text`、`notification_key`

`EventStore.stats()` 提供行数、文件大小、各类事件计数、解析错误数等。

## CollectorPreferences

使用 `EncryptedSharedPreferences` 存储：

- action socket token
- upload endpoint / API key
- 采集源开关
- collector 运行状态
- foreground package / class

Token 生成规则：

- Debug 构建：优先读取 `debug.dipecs.token` system property，否则使用固定 dev token。
- Release 构建：使用 `SecureRandom` 生成 64 位十六进制随机串。

首次访问时会把旧版明文 `SharedPreferences` 迁移到加密存储并清空旧文件。

## AuthorizedActionSocketServer

- 监听 `127.0.0.1:46321`（可配置）。
- 最大并发 client 线程 4，等待队列 16。
- 单 payload 上限 64 KiB，读超时 5 秒。
- 5 次认证失败后进入 30 秒 backoff。

处理流程：

1. 读取 payload。
2. 解析 JSON。
3. `message_type == "execute"` → 走 BridgeExecuteProtocol。
4. 其他 `message_type` → 校验 token、freshness window、HMAC。
5. 通过 `ActionExecutorBridge.dispatchAuthorizedActionJson` 执行。
6. 返回 JSON 状态。

### BridgeExecuteProtocol

Execute envelope 字段：

- `message_type`: `"execute"`
- `issued_at_ms`, `expires_at_ms`
- `action`: serialized `AuthorizedAction` 字符串
- `auth.hmac_sha256`

HMAC 输入：

```text
dipecs.android.bridge.execute.v1
issued_at_ms:<issued>
expires_at_ms:<expires>
action:<utf8-byte-len>:<serialized AuthorizedAction JSON>
```

 freshness 窗口默认 60 秒，允许 ±30 秒时钟 skew。

## ActionExecutorBridge 分发

| ActionType | 默认 target | 实际执行 |
| --- | --- | --- |
| `PreWarmProcess` | `own:resources` | `SystemActionExecutors.prewarmProcess` |
| `PrefetchFile` | 必须提供 | `SystemActionExecutors.prefetchFile` |
| `KeepAlive` | `work:collector_heartbeat` | `SystemActionExecutors.keepAlive` |
| `ReleaseMemory` | — | `SystemActionExecutors.releaseMemory` |
| `NoOp` | — | `SystemActionExecutors.noOp` |

`SystemActionExecutors` 在 platform-signed / root 环境下会启用系统级能力；普通 App 模式下会诚实回退或失败。

## AndroidManifest 要点

普通 App 可用的权限：

- `FOREGROUND_SERVICE`、`FOREGROUND_SERVICE_DATA_SYNC`
- `INTERNET`、`ACCESS_NETWORK_STATE`、`POST_NOTIFICATIONS`
- `PACKAGE_USAGE_STATS`
- `RECEIVE_BOOT_COMPLETED`（普通 App 不生效）

需要 platform/priv-app 签名才有效的权限：

- `START_ACTIVITIES_FROM_BACKGROUND`
- `INTERACT_ACROSS_USERS`
- `MANAGE_ACTIVITY_TASKS`
- `DELETE_CACHE_FILES`、`CLEAR_APP_CACHE`
- `WRITE_OOM_SCORE_ADJ`
- `SET_PROCESS_LIMIT`

## 调试 Android 端

常用 adb 命令：

```bash
# 查看 collector 日志
adb logcat -s dipecs:D

# 确认 service 运行
adb shell dumpsys activity services com.dipecs.collector/.services.CollectorForegroundService

# 导出 trace
adb shell run-as com.dipecs.collector cat files/traces/actions.jsonl

# 端口转发
adb forward tcp:46321 tcp:46321
```

## 相关文档

- [Android 采集器](collector.md)
- [Android 动作实现手册](action-bridge.md)
- [Android 安全与隐私边界](security-privacy.md)
- [调试指南](../team/debugging.md)
