# Android 安全与隐私边界

本文档记录当前 Android collector、导出、上传、prefetch 和 action socket 的安全边界。

## 默认原则

- 原始本地信号不直接进入模型后端。
- Android trace 是 app-private append-only JSONL。
- 落盘和导出的 trace 都经过 `EventStore.sanitizeForTrace`。
- `rawEvent` 是 Android 到 Rust production ingress 的唯一候选字段。
- `rawEvent: null` 行只用于 screening，不进入 Rust production replay。
- Web dashboard 只读取本地 sanitized JSONL 和 replay/audit 输出，不上传数据。

## 数据源边界

Production ingress:

- `UsageStatsManager` -> `RawEvent::AppTransition`
- `NotificationListenerService` -> `RawEvent::NotificationPosted` / `RawEvent::NotificationInteraction`
- `DeviceContext` -> `RawEvent::SystemState`

Screening:

- `AccessibilityService` 默认关闭。
- Accessibility 行默认 `rawEvent: null`，只用于验证 UI 信号是否值得提升为正式 schema。

## Trace 与导出

本地 trace 路径：

```text
<app-private-files>/traces/actions.jsonl
```

导出路径：

```text
/sdcard/Android/data/com.dipecs.collector/files/traces/actions.jsonl
```

脱敏规则覆盖：

- 通知标题/正文：`raw_title`、`raw_text` 清空。
- accessibility 文本：`text`、`textItems`、`sourceText`、`sourceContentDescription` 置空。
- socket/debug payload：`payload`、`responseBody` 置空。
- prefetch/action 目标：`target`、`cachePath` 置空。
- notification key/group/tag 等标识字段置空。

导出和清理都需要显式确认。清理会删除本地 JSONL trace 和 prefetch cache。

## 上传边界

上传只发送最近 100 条 sanitized events。

限制：

- periodic upload 默认关闭，必须打开 **Enable periodic upload**。
- 手动上传可用于验证，但仍只发送 sanitized events。
- endpoint 必须是 `https://`。
- endpoint 不允许解析到 localhost、private、link-local、multicast、IPv6 ULA 地址。
- 不跟随 HTTP redirect。
- 响应体读取上限为 16 KiB。
- UI 状态页只显示 endpoint 的 scheme/host 打码形式。

`llm` 模式下 API key 作为 bearer token 发送；token 存在 Android `EncryptedSharedPreferences`。

## Prefetch 边界

支持目标：

- `url:https://...`
- `uri:content://...`

限制：

- URL 只允许 HTTPS。
- 拒绝 localhost、private、link-local、multicast、IPv6 ULA 地址。
- 最多跟随 3 次 redirect，每次 redirect 后重新校验。
- 单次下载最多 2 MiB。
- 结果写入 app cache。
- cache TTL 为 24 小时。
- Clear 会删除 prefetch cache。

## Action Socket 边界

Android action socket:

- 只监听 `127.0.0.1`。
- token 存在 `EncryptedSharedPreferences`。
- UI 默认只显示 masked token。
- 复制 token 时标记为 sensitive clipboard 内容。
- 单 payload 上限为 64 KiB。
- 读超时为 5 秒。
- 多次认证失败会触发短时 backoff。

Ping:

- `send-authorized-action` CLI 当前是 health-check ping。
- ping 只要求 token，不 dispatch action。

真实 action dispatch:

- 必须由 Rust `ActionLifecycle` 生成 `AuthorizedAction`。
- `aios-action` 发送 `message_type: "execute"` envelope，包含
  `issued_at_ms`、`expires_at_ms`、serialized `AuthorizedAction` 字符串和
  `auth.hmac_sha256`。
- `auth.hmac_sha256` 是 HMAC-SHA256，覆盖 freshness window 和
  length-prefixed action JSON。
- Android 在 dispatch 前校验 freshness window、HMAC 和 action JSON。
- 缺 HMAC、HMAC 不匹配、过期、TTL 过长或 action malformed 都会拒绝。
- Android 返回 JSON status；Rust 只把 `status: "ok"` 视为 forwarded。

## Android-Safe Action Targets

当前 Android bridge 只接受以下低风险 action 语义：

| ActionType | 允许 target | Android 侧行为 |
| :--- | :--- | :--- |
| `PrefetchFile` | `url:https://...`, `uri:content://...` | 预取可访问内容到 app cache |
| `ReleaseMemory` | `cache:prefetch`, `cache:all`, 或空 target | 只清理 DiPECS 自己的 cache |
| `KeepAlive` | `work:*` 或空 target | 调度 DiPECS 自己的 `JobScheduler` 维护任务 |
| `PreWarmProcess` | `own:*` | 预热 DiPECS 自己的资源 |
| `PreWarmProcess` | `pkg:*`, `notif:*` | 发用户可见通知提示，不后台启动第三方 App |
| `NoOp` | 任意 | 只记录审计事件 |

不支持：

- 后台拉起第三方 Activity。
- 修改第三方进程优先级或 `oom_score_adj`。
- 清理第三方 App 内存或私有文件。
- 访问未授权的第三方 URI 或本机/内网 HTTP 目标。

## 文档与样本要求

可以提交：

- sanitized JSONL sample。
- replay audit artifact。
- daemon runtime trace artifact。
- dashboard 截图或本地分析结果。

不要提交：

- 原始通知正文。
- accessibility 原始文本。
- action socket token。
- API key。
- 私有 URI、cache path、用户文件名、联系人姓名。
