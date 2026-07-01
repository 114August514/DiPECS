# 安全模型与威胁假设

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `crates/aios-core/src/governance/`, `crates/aios-action/src/android_adapter.rs`, `apps/android-collector/.../actions/AuthorizedActionSocketServer.kt`

**这篇文档回答什么**：DiPECS 默认信任谁、哪些边界在阻止什么、以及已知的限制。  
**适合谁读**：评审架构、部署到真机或考虑引入新动作的人。

## TL;DR

- **可信基**：`PrivacyAirGap`、`PolicyEngine`、`ActionLifecycle`、`EncryptedSharedPreferences`。
- **核心不变量**：原始 PII 不越过 `PrivacyAirGap`；只有 `ActionLifecycle` 能构造 `AuthorizedAction`；Android bridge 通过 freshness window + HMAC 校验请求。
- **已知限制**：debug token 固定、action socket 监听 localhost、platform/root 模式可启用更强能力。

## 信任边界

```text
Android app (user-granted permissions)
  -> PrivacyAirGap (drop PII)
  -> StructuredContext (sanitized)
  -> DecisionRouter (produces IntentBatch only)
  -> PolicyEngine (per-action verdict)
  -> ActionLifecycle (seals AuthorizedAction)
  -> ActionAdapter (Offline / Default / Android)
  -> AuditRecord
```

每一层只能读取上一层允许它读取的数据，不能反向构造授权动作。

## 可信组件

| 组件 | 职责 | 为什么可信 |
| --- | --- | --- |
| `DefaultPrivacyAirGap` | 把 `RawEvent` 转成 `SanitizedEvent` | 纯函数；所有 PII 在此丢弃 |
| `WindowAggregator` | 聚合窗口上下文 | 只接触 `SanitizedEvent` |
| `PolicyEngine` | 风险/置信度/能力/target 审查 | 不构造 `AuthorizedAction` |
| `ActionLifecycle` | 唯一构造 `AuthorizedAction` 的入口 | `AuthorizedAction` 私有字段 + `pub(crate) seal` |
| `ActionAdapter` | 执行已授权动作 | 只能接收，不能伪造 |
| `EncryptedSharedPreferences` | 存储 Android token | Android Keystore 保护 |

## 不可伪造性

`AuthorizedAction` 位于 `aios-core::governance`：

- 字段私有，外部 crate 无法直接构造。
- `seal` 函数是 `pub(crate)`，只有 `aios-core` 内部能调用。
- 实现了 `Serialize`，但**没有**实现 `Deserialize`，因此不能从 JSON 反序列化伪造。

Android bridge 的 execute envelope 包含 serialized `AuthorizedAction` 字符串，
但 Android 侧只验证 HMAC 和 freshness，**不**重新 seal；它执行的是 Rust 已经 seal 过的动作描述。

## HMAC 与重放保护

`AndroidAdapter` 对每次 execute 请求计算 HMAC-SHA256：

```text
dipecs.android.bridge.execute.v1
issued_at_ms:<issued>
expires_at_ms:<expires>
action:<utf8-byte-len>:<serialized AuthorizedAction JSON>
```

- `issued_at_ms` 与 `expires_at_ms` 构成 freshness window，默认 60 秒。
- HMAC 覆盖 action JSON 内容，因此同一 token 不能用于不同 action。
- Android 侧独立重算并拒绝过期、过长 TTL、HMAC 不匹配或 malformed 的请求。

## Action Socket 边界

- 只监听 `127.0.0.1`。
- Token 来自 `EncryptedSharedPreferences`；debug 构建有固定 token，不能用于生产。
- payload 上限 64 KiB，读超时 5 秒，多次失败 backoff。
- `send-authorized-action` CLI 只是 ping，不派发动作；真实派发走 `send-action` 或 daemon pipeline。

## 部署模式与信任差异

| 部署模式 | 能力差异 | 威胁假设 |
| --- | --- | --- |
| 正常 App | 只能清理自身 cache、调度自己的 JobScheduler、提示用户 | 设备上其他 App 不可信；用户数据不可泄露 |
| Platform-signed / `/system/bin/dipecsd` | 可清第三方 cache、触发 `drop_caches`、启动第三方 launcher | 认为平台证书 / system image 可信；仍需审计每一次动作 |
| 开发主机 + `adb forward` | 通过 USB 调试通道；固定 debug token | 开发者控制设备；仅用于调试 |

## 已知限制

- **Debug token**：开发构建使用固定共享 token，不能用于生产环境。
- **localhost socket**：任何获得该 socket 访问权的本地进程都可能发送 ping；只有持有正确 token 才能通过 HMAC。
- **eBPF / fanotify stub**：当前没有真实实现，因此无法依赖它们做强制隔离。
- **AccessibilityService**：screening source，不进入生产 schema；若未来提升为生产源，需要额外审查。

## 隐私保证的验证

- `privacy_leak_test`：扫描输出中是否出现 raw text / file path。
- `privacy_airgap_property_test`：对所有 `RawEvent` 变体做生成式检查。
- `model_input_does_not_contain_raw_notification_text_or_file_path`：验证模型输入无 PII。

## 相关文档

- [动作治理](action-governance.md)
- [动作执行](action-execution.md)
- [Android 安全与隐私边界](../android/security-privacy.md)
- [Android 动作能力边界](../android/action-boundary.md)
- [Schema 参考](../refs/schemas.md)
