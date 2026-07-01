# 动作适配器

> Status: Current  > Last verified: 2026-07-01  > Code anchors: `crates/aios-action/src/lib.rs`, `crates/aios-action/src/android_adapter.rs`, `crates/aios-action/src/offline_adapter.rs`

**这篇文档回答什么**：`ActionAdapter` trait、三种适配器的区别、以及 daemon 如何选择适配器。  
**适合谁读**：需要新增动作执行路径或理解 replay / bridge / stub 差异的人。

## TL;DR

DiPECS 有三种 `ActionAdapter`：

- `OfflineAdapter`：纯离线、确定性，用于 replay 和 golden hash。
- `DefaultActionExecutor`：桌面/普通 Linux 下的纯 stub。
- `AndroidAdapter`：通过 localhost socket 转发到 Android app。

`ActionLifecycle` 只接收 `AuthorizedAction`，调用注入的 adapter。

## ActionAdapter trait

```rust
pub trait ActionAdapter {
    fn name(&self) -> &'static str;
    fn execute(
        &self,
        authorized: &AuthorizedAction,
    ) -> Result<ActionOutcome, AdapterError>;
}
```

关键不变量：adapter **不能**自己构造 `AuthorizedAction`。

## 适配器对比

| 适配器 | 输入 | 输出 | 使用场景 | 是否参与 golden hash |
| --- | --- | --- | --- | --- |
| `OfflineAdapter` | `AuthorizedAction` | 确定性 `ActionOutcome` | `aios-cli replay` | 是 |
| `DefaultActionExecutor` | `AuthorizedAction` | 纯 tracing stub | daemon 未启用 bridge 时 | 否（不在 hash 路径） |
| `AndroidAdapter` | `AuthorizedAction` | 设备真实执行结果 | daemon 启用 Android bridge | 否（设备结果非确定） |

## OfflineAdapter

- 不访问文件系统、网络或环境变量。
- 每个 `ActionType` 返回固定 summary，如 `simulate_prewarm:pkg`。
- `latency_us` 恒为 0。
- 是 golden hash 路径中唯一的执行器，保证可复现。

## DefaultActionExecutor

- 桌面/普通 Linux 下的默认适配器。
- 每个 action 只打 `tracing::info`，不执行真实系统调用。
- 用于本地 daemon 开发、测试和 CI。

## AndroidAdapter

启用条件：

```bash
DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=true
DIPECS_ANDROID_ACTION_BRIDGE_HOST=127.0.0.1
DIPECS_ANDROID_ACTION_BRIDGE_PORT=46321
DIPECS_ANDROID_ACTION_BRIDGE_TOKEN=<token>
```

构造时注入配置；`execute()` 内不再读取环境变量。

### 转发路由

| ActionType | 是否转发 | 条件 |
| --- | --- | --- |
| `PrefetchFile` | 是 | target 以 `url:` 或 `uri:` 开头 |
| `PreWarmProcess` | 是 | 无条件 |
| `KeepAlive` | 是 | 无条件 |
| `ReleaseMemory` | 是 | 无条件 |
| `NoOp` | 否 | 本地 stub |

不满足转发条件的 `PrefetchFile` 回退到 `DefaultActionExecutor`。

### 诚实Outcome

`AndroidAdapter` 是请求/响应模型：

- 连接、写请求、半关写端、读响应。
- 超时、连接被拒、空响应、HMAC 失败都返回 `Err` → `ActionState::Failed`。
- 只有设备返回 `"status": "ok"` 才视为成功。

### HMAC 输入

```text
dipecs.android.bridge.execute.v1
issued_at_ms:<issued>
expires_at_ms:<expires>
action:<utf8-byte-len>:<serialized AuthorizedAction JSON>
```

## Daemon 如何选择适配器

`crates/aios-daemon/src/lib.rs` 在启动时：

```rust
if let Some(config) = AndroidBridgeConfig::from_env() {
    AndroidAdapter::new(config)
} else {
    DefaultActionExecutor::new()
}
```

`aios-cli replay` 则固定使用 `OfflineAdapter`。

## 相关文档

- [动作治理](action-governance.md)
- [Android 动作实现手册](../android/action-bridge.md)
- [Android 动作能力边界](../android/action-boundary.md)
- [安全模型与威胁假设](security-model.md)
