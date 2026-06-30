# Android 动作实现手册

> Status: Current  
> Last verified: 2026-06-30

本文记录当前已经落地的 Android action bridge。更早文档中关于 WorkManager、通用 ActionResult、静默预热第三方应用的内容已经过期。

## 相关文件

| 文件 | 职责 |
| --- | --- |
| `crates/aios-action/src/lib.rs` | `DefaultActionExecutor`、Android bridge env、target 白名单、HMAC payload。 |
| `apps/android-collector/.../actions/AuthorizedActionSocketServer.kt` | localhost socket、token、TTL、HMAC、rate limit。 |
| `apps/android-collector/.../actions/ActionExecutorBridge.kt` | action type 分发。 |
| `apps/android-collector/.../actions/AccessibleContentPrefetcher.kt` | `PrefetchFile` 实现。 |
| `apps/android-collector/.../actions/ActionMaintenanceScheduler.kt` | `KeepAlive` 实现。 |
| `apps/android-collector/.../actions/CacheTrimmer.kt` | `ReleaseMemory` 实现。 |
| `apps/android-collector/.../actions/OwnResourceWarmer.kt` | `PreWarmProcess(own:*)` 实现。 |
| `apps/android-collector/.../actions/UserVisibleActionNotifier.kt` | 第三方 app 相关提示。 |

## 启用 Rust -> Android 转发

```bash
DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=true
DIPECS_ANDROID_ACTION_BRIDGE_HOST=127.0.0.1
DIPECS_ANDROID_ACTION_BRIDGE_PORT=46321
DIPECS_ANDROID_ACTION_BRIDGE_TOKEN=<token-from-android-app>
```

未启用时，`DefaultActionExecutor` 只返回本地 stub outcome。

## Payload 字段

`aios-action` 序列化 `AuthorizedAction` 后追加：

```json
{
  "auth_token": "...",
  "issued_at_ms": 0,
  "expires_at_ms": 0,
  "action_signature": "hex-hmac-sha256"
}
```

签名输入是 length-prefixed canonical string：

```text
dipecs.android.action.v1
issued_at_ms:<issued>
expires_at_ms:<expires>
action_type:<len>:<ActionType>
target:<len>:<target>
urgency:<len>:<Urgency>
```

Android 侧用相同规则验证 HMAC。

## Action dispatch

### `PrefetchFile`

Allowed targets：

```text
url:https://...
uri:content://...
```

行为：

- `url:` 使用 `HttpURLConnection` 下载。
- `uri:` 使用 `ContentResolver.openInputStream`。
- 写入 `cacheDir/prefetch`。
- 最大 2 MiB。
- 自动清理 24 小时前的 prefetch cache。
- HTTPS host 和 redirect target 都做私有地址过滤。

### `KeepAlive`

Allowed targets：

```text
work:*
None -> work:collector_heartbeat
```

行为：

- 使用 `JobScheduler` 调度 `ActionMaintenanceJobService`。
- 只调度 DiPECS-owned maintenance job。

### `ReleaseMemory`

Allowed targets：

```text
cache:prefetch
cache:all
None -> cache:prefetch
```

行为：

- `cache:prefetch` 清理 prefetch cache。
- `cache:all` 清理 app-owned cache children。
- 不触碰第三方进程或第三方文件。

### `PreWarmProcess`

Allowed targets：

```text
own:*
pkg:*
notif:*
None -> own:resources
```

行为：

- `own:*` 调用 `OwnResourceWarmer`，准备 DiPECS 自身 cache / token / trace stats。
- `pkg:*` 和 `notif:*` 不后台启动第三方 app，只发布用户可见 action hint。

### `NoOp`

不转发或只记录 no-op acknowledgement。

## 手动验证

1. Android app 打开 collector service。
2. 复制 action socket token。
3. 端口转发到设备 socket。
4. 启动 `dipecsd` 并设置 Android bridge env。
5. 用能产生低风险 action 的 trace 或 replay 验证 `AuditRecord`。

CLI socket 命令当前只验证 token 和 socket 可达性：

```bash
cargo run -p aios-cli -- send-authorized-action \
  --auth-token <token> \
  --host 127.0.0.1 \
  --port 46321
```

它不会派发 prefetch 或其他动作。
