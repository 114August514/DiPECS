# Android 动作实现手册

> Status: Current  
> Last verified: 2026-07-05

本文记录当前已经落地的 Android action bridge。更早文档中关于 WorkManager、通用 ActionResult、静默预热第三方应用的内容已经过期。

Deferred items for v0.2:

- `SuggestedAction.params` remains deferred until an action needs structured arguments beyond `target`.
- SAF URI prefetch is implemented through `uri:content://...` targets.
- `DipecsCollectorApp` is won't-do for v0.2 because the current services and repositories do not need process-wide lifecycle hooks.
- WorkManager remains later work; the current prototype accepts foreground service plus bounded single-thread executors.

## 相关文件

| 文件 | 职责 |
| --- | --- |
| `crates/aios-action/src/lib.rs` | `DefaultActionExecutor` — 纯确定性 stub，不访问网络、环境变量或 Android。 |
| `crates/aios-action/src/android_adapter.rs` | `AndroidAdapter` — Android bridge env、target 白名单、HMAC 载荷、TCP 请求/响应协议。 |
| `crates/aios-action/src/offline_adapter.rs` | replay / golden 使用的 deterministic adapter。 |
| `apps/android-collector/.../actions/AuthorizedActionSocketServer.kt` | localhost socket、token、TTL、HMAC、rate limit。 |
| `apps/android-collector/.../actions/ActionExecutorBridge.kt` | action type 分发。 |
| `apps/android-collector/.../actions/AccessibleContentPrefetcher.kt` | `PrefetchFile` 实现。 |
| `apps/android-collector/.../actions/ActionMaintenanceScheduler.kt` | `KeepAlive` 实现。 |
| `apps/android-collector/.../actions/CacheTrimmer.kt` | 正常 App 模式下 `ReleaseMemory(cache:*)` 实现。 |
| `apps/android-collector/.../actions/VolatileMemoryCache.kt` | `own:volatile-cache:*` seed 与 `cache:volatile` 释放的 app-owned 可丢弃内存缓存。 |
| `apps/android-collector/.../actions/SystemActionExecutors.kt` | platform/root 模式下的系统级执行器。 |
| `apps/android-collector/.../actions/SystemPrewarmActivity.kt` | `PreWarmProcess(own:*)` / 系统级预热 activity。 |
| `apps/android-collector/.../actions/UserVisibleActionNotifier.kt` | 用户可见动作提示。 |

## 启用 Rust -> Android 转发

```bash
DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=true
DIPECS_ANDROID_ACTION_BRIDGE_HOST=127.0.0.1
DIPECS_ANDROID_ACTION_BRIDGE_PORT=46321
DIPECS_ANDROID_ACTION_BRIDGE_TOKEN=<token-from-android-app>
```

未启用时，daemon 启动期选择 `DefaultActionExecutor`（纯本地 stub），不包含 Android bridge 逻辑。

## Execute Envelope

`aios-action` 序列化 `AuthorizedAction` 后，把 JSON 字符串放入 execute
envelope：

```json
{
  "message_type": "execute",
  "issued_at_ms": 0,
  "expires_at_ms": 0,
  "action": "{\"action\":{...}}",
  "auth": {
    "hmac_sha256": "hex-hmac-sha256"
  }
}
```

签名输入是 length-prefixed canonical string：

```text
dipecs.android.bridge.execute.v1
issued_at_ms:<issued>
expires_at_ms:<expires>
action:<utf8-byte-len>:<serialized AuthorizedAction JSON>
```

Android 侧用相同规则验证 HMAC、freshness window 和 action JSON。dispatch 后
返回 JSON status；Rust 只把 `status: "ok"` 映射为 forwarded outcome，并把
Android 返回的 `summary` / `latency_us` 写入 `ActionOutcome`。

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
- platform/root 模式下还会降低自身 OOM score 并尝试把自身 PID 写入 foreground cpuset。

### `ReleaseMemory`

Allowed targets：

```text
cache:prefetch
cache:all
cache:volatile
pkg:<package>
page
None -> cache:prefetch
```

行为：

- `cache:prefetch` 清理 prefetch cache。
- `cache:all` 在正常 App 模式下清理 app-owned file cache，并释放 app-owned volatile cache；platform/root 模式下还会尝试清理所有应用缓存。
- `cache:volatile` 释放由 `PreWarmProcess own:volatile-cache:<MB>` seed 的进程内可丢弃内存缓存；这是 #99 正收益证据使用的默认安全语义。
- `pkg:<package>` 需要 platform/root，对指定包执行 `pm clear --cache-only`。
- `page` 需要 root，向 `/proc/sys/vm/drop_caches` 写入 `1`，触发全局 page cache 回收。

### `PreWarmProcess`

Allowed targets：

```text
own:*
own:volatile-cache:<MB>
pkg:*
notif:*
None -> own:resources
```

行为：

- `own:*` 调用 `SystemPrewarmActivity`，准备 DiPECS 自身资源。
- `own:volatile-cache:<MB>` 在进程内 seed 一个有上限的 app-owned volatile cache，用于后续 `ReleaseMemory cache:volatile` 在真实内存压力下释放。
- `pkg:*` 和 `notif:*` 在正常 App 模式下只发布用户可见 action hint；platform/root 模式下会启动目标包的 launcher Activity 并立即 finish task，触发 Zygote fork。

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

要派发一个真实的签名动作（注意这会真实执行）：

```bash
cargo run -p aios-cli -- send-action \
  --auth-token <token> \
  --host 127.0.0.1 \
  --port 46321 \
  --action-type KeepAlive \
  --target work:collector_heartbeat \
  --urgency IdleTime
```

`send-action` 会构造完整的 HMAC-signed execute envelope 并发送给 Android bridge。
