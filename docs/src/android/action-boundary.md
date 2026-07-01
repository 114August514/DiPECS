# Android 动作能力边界

> Status: Current  
> Last verified: 2026-06-30  
> Current implementation: `aios-action` + `apps/android-collector/.../actions`

DiPECS 的动作层遵守两个边界：

1. Rust 侧只执行 `ActionLifecycle` seal 后的 `AuthorizedAction`。
2. Android 侧只使用公开 API，并把动作限制在 DiPECS 自身资源、用户授权内容或用户可见提示上。

## 当前执行路径

```text
IntentBatch
  -> PolicyEngine
  -> ActionLifecycle
  -> AuthorizedAction
  -> DefaultActionExecutor
  -> Android localhost socket (optional)
  -> ActionExecutorBridge
  -> Android-safe implementation
```

`DefaultActionExecutor` 默认执行本地 stub。只有设置：

```bash
DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=true
DIPECS_ANDROID_ACTION_BRIDGE_TOKEN=<token>
```

并且 action target 通过白名单检查时，才会转发到 Android socket。

## Socket 安全边界

Android socket 只监听 `127.0.0.1`。CLI ping payload 只用于 health-check，
必须包含 `auth_token`，不会派发动作。

`aios-action` 的真实 dispatch 使用 execute envelope：

- `message_type: "execute"`
- `issued_at_ms`
- `expires_at_ms`
- `action`，内容是 serialized `AuthorizedAction` 字符串。
- `auth.hmac_sha256`

`auth.hmac_sha256` 是 HMAC-SHA256，覆盖 freshness window 和 length-prefixed
action JSON。Android 侧还限制 payload 大小、读超时、失败退避、最大 client
数和 envelope TTL，并返回 JSON status 给 Rust。

CLI 的 `send-authorized-action` 当前只是 ping/health-check，不派发动作。

## 当前 Android-safe actions

| ActionType | 可转发 target | Android 实现 | 边界 |
| --- | --- | --- | --- |
| `PrefetchFile` | `url:https://...` / `uri:content://...` | `AccessibleContentPrefetcher` | 只预取 HTTPS 或 app 可访问 content URI。 |
| `KeepAlive` | `work:*` / `None` | `ActionMaintenanceScheduler` | 只调度 DiPECS-owned `JobScheduler` job。 |
| `ReleaseMemory` | `cache:prefetch` / `cache:all` / `None` | `CacheTrimmer` | 只删除 DiPECS-owned cache。 |
| `PreWarmProcess` | `own:*` / `pkg:*` / `notif:*` | `OwnResourceWarmer` / `UserVisibleActionNotifier` | `own:*` 只预热自身资源；第三方 app 只发用户可见提示。 |
| `NoOp` | 无 | local stub / Android no-op record | 无副作用。 |

## 不支持的动作语义

当前不会做：

- 静默预热第三方应用进程。
- 后台强拉第三方 Activity。
- 修改第三方进程 `oom_score_adj`。
- 清理第三方应用内存。
- 读取第三方私有文件。
- 绕过用户授权访问 content URI。

## Prefetch 限制

`url:` target：

- 只允许 `https://`。
- 拒绝 localhost、private、link-local、multicast、unique-local IPv6。
- redirect 后重新校验目标。
- 最大下载 2 MiB。
- cache 24 小时 TTL。

`uri:` target：

- 只允许 `content://`。
- 实际读取依赖 Android `ContentResolver` 权限。
- 同样写入 app cache 并受大小限制。

## 与 ActionType 原始命名的关系

当前仍保留 `PreWarmProcess`、`KeepAlive`、`ReleaseMemory` 等协议名，以减少跨 crate 变更。但 Android 实现语义已经收缩：

- `PreWarmProcess` 不是第三方进程预热。
- `KeepAlive` 不是进程保活开关。
- `ReleaseMemory` 不是系统级内存回收。

这些名字如果未来继续产品化，应该通过新 RFC 替换为更准确的 action taxonomy。
