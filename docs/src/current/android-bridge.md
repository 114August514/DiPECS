# Android 桥接

> Status: Current  
> Last verified: 2026-06-30

Android 侧有两条桥接方向：

1. Android public API 采集 -> JSONL -> Rust daemon。
2. Rust 授权动作 -> localhost socket -> Android action executor。

## 采集桥接

Android collector 写入：

```text
<app-private-files>/traces/actions.jsonl
```

每行包含人类可读字段和可选 `rawEvent`。Rust 只消费符合 `aios-spec::RawEvent` 外部标签格式的 `rawEvent`。

Promoted sources：

- `UsageStatsManager` -> `AppTransition` / `ScreenState`
- `NotificationListenerService` -> `NotificationPosted` / `NotificationInteraction`
- `DeviceContext` heartbeat -> `SystemState`

Screening source：

- `AccessibilityService` 目前不进生产 Rust 管线；`rawEvent: null` 会被跳过。

## 动作 socket

Android `AuthorizedActionSocketServer` 监听：

```text
127.0.0.1:46321
```

基础保护：

- ping payload 需要 `auth_token`，并做常量时间比较。
- Rust `aios-action` dispatch 使用 `message_type: "execute"` envelope。
- payload 最大 64 KiB。
- 读超时。
- auth 失败退避。
- 最大 client 线程和 pending client 限制。
- execute envelope 需要 `issued_at_ms` / `expires_at_ms` freshness window。
- execute envelope 需要 `auth.hmac_sha256`，覆盖 freshness window 和
  length-prefixed serialized `AuthorizedAction`。
- Android 返回 JSON status，Rust 只把 `status: "ok"` 映射为 forwarded。

CLI 的 `send-authorized-action` 当前只是 ping/health-check，不派发动作。

## Android-safe actions

Rust `aios-action` 只把符合目标前缀白名单的动作转发到 Android：

| ActionType | Android target 约定 | Android 行为 |
| --- | --- | --- |
| `PrefetchFile` | `url:https://...` / `uri:content://...` | 下载或读取可访问内容，写入 app cache。 |
| `KeepAlive` | `work:*` 或 `None` | 调度 DiPECS-owned `JobScheduler` maintenance job。 |
| `ReleaseMemory` | `cache:prefetch` / `cache:all` / `None` | 只清理 DiPECS-owned cache。 |
| `PreWarmProcess` | `own:*` / `pkg:*` / `notif:*` | `own:*` 预热自身资源；`pkg:*` / `notif:*` 发布用户可见提示。 |
| `NoOp` | 无 | 不转发或只记录 no-op。 |

网络预取限制：

- 只允许 `https://`。
- 拒绝 localhost、private、link-local、multicast、unique-local IPv6。
- redirect 目标会重新校验。
- 最大下载 2 MiB。
- prefetch cache 默认 24 小时 TTL。

这些限制保证动作层遵守 Android 公开 API 边界：不静默预热第三方进程，不修改第三方进程保活参数，不清理第三方内存。
