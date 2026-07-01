# Android 真机验证手册

> Deprecated: 2026-06-30. The current v0.2 validation target is emulator plus sanitized JSONL replay, not physical-device sign-off. Keep this file as historical reference only; do not treat it as a required release checklist unless real-device validation is reintroduced.

本文档把真机 Android 采集验证分成两层：

1. Android App 内的用户/开发者页面，用于授权、采集、隐私确认、导出、清理和 action socket token 管理。
2. 本地开发者侧 replay/audit 验证，用于确认导出的 sanitized JSONL 能进入 Rust pipeline。

## 验证目标

一次合格的真机验证需要证明：

- App 能在真实 Android 设备上采集 Tier 0 public API 事件。
- `UsageStatsManager`、`NotificationListenerService`、`DeviceContext` 能产生非空 `rawEvent`。
- `AccessibilityService` 默认关闭；即使启用，也只作为 screening source，`rawEvent: null` 行不会进入 Rust production replay。
- 导出的 JSONL 是 sanitized copy，不包含通知正文、accessibility 文本、socket payload、cache path、action target 或 token。
- `aios-cli replay` 能消费导出的 JSONL，并产生 policy/audit 输出。
- action socket 只监听 `127.0.0.1`，payload 必须带 token；真实 action 还必须带 freshness window 和 HMAC-SHA256 `action_signature`。
- 上传和 prefetch 不允许访问 localhost/private/link-local/multicast 地址；上传不跟随重定向，prefetch 会重新校验重定向目标。

## 前置条件

Android 设备：

- Android 13+ 推荐；当前 app 最低支持 Android 8。
- 已安装 debug APK。
- 已启用开发者选项和 USB debugging。

本地开发机：

- `adb` 可用，且 `adb devices` 能看到设备。
- Rust workspace 能运行 `cargo run -p aios-cli`。
- 如需本地构建 APK，配置 Android SDK Platform 35。

## 设备侧流程

1. 安装并打开 `apps/android-collector`。
2. 确认 `AccessibilityService` 默认 disabled。
3. 授权 Usage Access。
4. 授权 Notification Listener。
5. Android 13+ 授权 Post Notifications。
6. 保持 `UsageStatsManager`、`NotificationListenerService`、`DeviceContext heartbeat` enabled。
7. 点击 `Start Collector`。
8. 切换到另一个 App，再切回 DiPECS。
9. 触发一条无敏感内容的测试通知。
10. 等待至少一次 DeviceContext heartbeat。
11. 回到 DiPECS 页面，检查 Trace preview。
12. 点击 `Export JSONL Trace` 并确认导出。

## App 页面验收点

| 项目 | 合格标准 |
| :--- | :--- |
| Usage access | `enabled` |
| Notification listener | `enabled` |
| Post notifications | Android 13+ 显示 `enabled` |
| AccessibilityService | 默认 `disabled`，只在 screening 时手动开启 |
| Collector service | `running` |
| Last DeviceContext heartbeat | 有非空时间 |
| Action socket | service 启动后显示 listening 或明确错误 |
| Trace events | 大于 0 |
| Production rawEvent rows | 大于 0 |
| rawEvent kinds | 至少出现 `AppTransition`、`NotificationPosted`、`SystemState` 中的实际触发项 |
| Screening/rawEvent-null rows | 可以大于 0，但不应阻塞 replay |
| Schema status | `production replay candidate` 或 `mixed production and screening rows` |
| Last export | 显示导出路径和时间 |

## 拉取导出 Trace

App 页面会生成实际命令。默认路径如下：

```bash
adb pull /sdcard/Android/data/com.dipecs.collector/files/traces/actions.jsonl \
  data/traces/android_real_device_sample.redacted.jsonl
```

提交仓库前必须确认该文件是 sanitized 版本。不要提交包含真实通知正文、accessibility 文本、socket payload、cache path、action target、token、私有 URI 或联系人信息的 trace。

## Rust Replay

运行 policy replay：

```bash
cargo run -p aios-cli -- replay \
  data/traces/android_real_device_sample.redacted.jsonl \
  --stages policy \
  --audit data/evaluation/android_real_device.audit.ndjson
```

验收点：

- replay 输出最后一行为 `stage = "summary"`。
- `lines_parse_error = 0`。
- `events_ingested > 0`。
- `lines_skipped_no_raw_event` 只对应 screening 行。
- audit 文件不包含原始通知正文或 accessibility 文本。

## Daemon Ingress

用导出的 JSONL 验证 daemon 入口：

```bash
cargo run -p aios-daemon --bin dipecsd -- \
  --no-daemon \
  --android-trace-jsonl data/traces/android_real_device_sample.redacted.jsonl \
  --trace-output data/evaluation/android_real_device.runtime.ndjson
```

验收点：

- daemon 能读取 append-only Android JSONL。
- runtime trace 中出现窗口级记录。
- `rawEvent: null` 行不会进入 production pipeline。

## 本地 Dashboard

可以直接用浏览器打开：

```text
tools/trace-dashboard/index.html
```

加载两个文件：

- Android sanitized JSONL trace。
- `aios-cli replay` 输出或 audit NDJSON。

Dashboard 只做本地只读解析，不上传任何内容。默认只展示事件类型、package、`rawEvent` kind、replay stage 和 policy/audit 摘要；不渲染通知正文、accessibility 文本、token、payload、cache path 或 action target。

## Upload 与 Prefetch 边界

上传：

- periodic upload 默认关闭，必须显式打开。
- 手动上传也只发送最近 100 条 sanitized JSONL events。
- endpoint 必须是 `https://`。
- endpoint 不允许解析到 localhost/private/link-local/multicast/IPv6 ULA 地址。
- 不跟随 HTTP redirect。
- `llm` 模式下 API key 作为 bearer token 发送；状态页只显示 endpoint 打码形式。

prefetch：

- 只接受 `url:https://...` 或持久授权的 `uri:content://...`。
- 拒绝 localhost/private/link-local/multicast/IPv6 ULA 地址。
- 最多跟随 3 次 redirect，每次 redirect 后重新校验目标。
- 单次下载上限 2 MiB。
- 缓存写入 app cache，24 小时 TTL；点击 Clear 会同时删除 trace 和 prefetch cache。

其他 Android-safe action：

- `ReleaseMemory(cache:prefetch)` 应删除 DiPECS 自己的 prefetch cache，并记录 `release_memory_completed`。
- `KeepAlive(work:collector_heartbeat)` 应调度 `JobScheduler` 维护任务，并记录 `keep_alive_scheduled` 和 `keep_alive_job_executed`。
- `PreWarmProcess(own:resources)` 应预热 DiPECS 自己的 trace/cache/token 相关资源，并记录 `own_resources_prewarmed`。
- `PreWarmProcess(pkg:* 或 notif:*)` 应只发用户可见通知提示，不后台启动第三方 App。

## Action Socket

1. 在 App 中复制完整 action socket token。
2. 转发端口：

```bash
adb forward tcp:46321 tcp:46321
```

3. 发送 health-check ping：

```bash
cargo run -p aios-cli -- send-authorized-action \
  --auth-token <copied-token> \
  --host 127.0.0.1 \
  --port 46321
```

安全验收点：

- 不带 token 的 payload 必须失败。
- 错误 token 必须失败。
- socket 只监听 `127.0.0.1`。
- App trace 只记录受限诊断信息，不记录 token 或原始 payload。
- CLI ping 不 dispatch 动作。
- 真正的 prefetch 动作必须由 Rust `ActionLifecycle` 授权后通过 `aios-action` 转发。
- Android 侧会校验 `issued_at_ms`、`expires_at_ms` 和 `action_signature`，拒绝过期、缺签名或签名不匹配的 action payload。
- socket 转发的 `ReleaseMemory`、`KeepAlive`、`PreWarmProcess` 也必须使用上述 Android-safe target 前缀。

## 交付材料

一次完整真机验证应提交：

- App 页面截图：权限状态、运行状态、Trace preview、Privacy boundary。
- sanitized JSONL 样本，推荐文件名 `data/traces/android_real_device_sample.redacted.jsonl`。
- replay audit artifact。
- daemon runtime trace artifact。
- action socket 成功/失败验证说明。
- 如启用 prefetch，说明目标类型、缓存清理方式和是否触发 TTL。

不要提交：

- 原始未脱敏通知正文。
- Accessibility 原始文本。
- action socket token。
- 私有 URI、cache path、用户文件名或联系人姓名。
