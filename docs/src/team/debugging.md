# 调试指南

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `crates/aios-daemon/src/lib.rs`, `crates/aios-action/tests/android_bridge_e2e_test.rs`, `tests/scenarios/lib/`

**这篇文档回答什么**：当 `dipecsd` 没输出、action-loop 失败或 golden hash 变化时，如何分层定位问题。  
**适合谁读**：正在本地运行或真机调试 DiPECS 的开发者。

## TL;DR

排错按分层顺序：

1. 先看日志和 runtime NDJSON trace。
2. 再用 mock-socket 测试验证 action-loop 本身。
3. 最后上真机/模拟器脚本，对照故障模式表。

## 何时读这篇

| 现象 | 看哪一节 |
| --- | --- |
| `cargo test` 绿但 `dipecsd` 无输出 | 日志级别 |
| 不确定决策/审计发生了什么 | 读取 runtime NDJSON trace |
| Android bridge 连不上 | 连接 Android bridge |
| action-loop 失败 | 用 mock-socket 测试 / 端到端故障模式 |
| golden trace hash 变化 | 端到端脚本常见故障模式 |

## 排错前 checklist

- [ ] 已设置合适的 `RUST_LOG` 级别
- [ ] `--trace-output` 已指定（离线调试）
- [ ] `adb forward tcp:46321 tcp:46321` 已建立（真机/模拟器）
- [ ] token 一致且设备端已 `pm clear` 后重新启动 app
- [ ] mock-socket 测试已通过

## 日志级别

`dipecsd` 默认 `dipecs=info`：

```bash
# 开发默认
RUST_LOG=info cargo run -p aios-daemon --bin dipecsd -- --no-daemon

# 查看决策、窗口、策略、执行细节
RUST_LOG=dipecs=debug cargo run -p aios-daemon --bin dipecsd -- --no-daemon

# 只跟踪关键事件
RUST_LOG=dipecs=trace cargo run -p aios-daemon --bin dipecsd -- --no-daemon 2>&1 \
  | grep -E "sanitize|window|policy|execute"

# 只看延迟
RUST_LOG=dipecs=info cargo run -p aios-daemon --bin dipecsd -- --no-daemon 2>&1 \
  | grep "latency"
```

## 读取 runtime NDJSON trace

```bash
RUST_LOG=info cargo run -p aios-daemon --bin dipecsd -- \
  --no-daemon \
  --android-trace-jsonl path/to/actions.jsonl \
  --trace-output data/evaluation/runtime.ndjson
```

每行包含窗口元数据、raw event 统计、context summary、decision route、audit records。

常用 `jq` 过滤：

```bash
jq 'select(.decision.route == "CloudLlm") | {window_id, latency_us, error}' data/evaluation/runtime.ndjson
jq 'select(.audit_records | length > 0)' data/evaluation/runtime.ndjson
```

## 连接 Android bridge

Android app 在设备内部监听 `127.0.0.1:46321`：

```bash
adb forward tcp:46321 tcp:46321
```

启用 bridge：

```bash
export DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=1
export DIPECS_ANDROID_ACTION_BRIDGE_HOST=127.0.0.1
export DIPECS_ANDROID_ACTION_BRIDGE_PORT=46321
export DIPECS_ANDROID_ACTION_BRIDGE_TOKEN=dipecs-dev-emulator-shared-token-00000000
```

如需覆盖调试 token：

```bash
adb shell setprop debug.dipecs.token my-local-debug-token
adb shell pm clear com.dipecs.collector
```

## 用 mock-socket 测试

最快的回路验证：

```bash
cargo test -p aios-action android_bridge_e2e_test
```

它会在本地起 `TcpListener` 模拟 Android bridge，并验证：

- 信封字段：`message_type`、`issued_at_ms`、`expires_at_ms`、`auth.hmac_sha256`、`action`。
- freshness 窗口固定 60 秒。
- HMAC 基于 canonical 字符串重算：

```text
dipecs.android.bridge.execute.v1
issued_at_ms:{issued_at_ms}
expires_at_ms:{expires_at_ms}
action:{}:{action}
```

- 拒绝错误 HMAC、过期信封、重放信封。

CLI 层还有 `crates/aios-cli/tests/android_adapter_test.rs`。

## 端到端脚本常见故障模式

| 现象 | 可能原因 | 处理 |
| --- | --- | --- |
| `authorized_action_socket_empty` | `adb forward` 数据/FIN 竞态；host 写完立即关闭，FIN 先于 payload 到达 app | 查看 `tests/scenarios/lib/action-loop-stages.sh` forensic 输出；用 `action-forensic-sender.py` 手动重放 |
| `NOT-EXECUTED` | token 不匹配、HMAC/freshness 失败、`adb forward` 未建立、前台服务未运行 | 核对 token、确认 `adb forward`、检查 app 日志中的 HMAC 拒绝 |
| `SCHEDULED` | JobScheduler 已入队但尚未执行 | 增大 `EXEC_SETTLE_SECS`，等待后重新 pull trace |
| `cargo test` 绿但 `dipecsd` 无输出 | mpsc channel 发送端提前 drop，processing task 未启动 | 用 `RUST_LOG=debug` 启动，确认出现 `processing task started` |
| 跨 run 旧 trace / redaction leak | 未 `pm clear` 或安装了旧 APK | 手动 `adb shell pm clear com.dipecs.collector` |
| golden hash 变化 | replay 过程中引入非确定性 timestamp / UUID | 对比新旧 `audit_hash`，定位非确定性字段 |
| 设备内 `dipecsd` 启动失败 | ABI 错误、NDK linker 不匹配、app socket 未监听 | 查看 `on-device-dipecsd-stages.sh` stage 1 smoke 返回码；stage 2 用 `/proc/net/tcp` 探测 socket |

## 相关文档

- [开发指南](dev.md)
- [环境配置](environment.md)
- [动作执行与 Android bridge](../architecture/action-execution.md)
- [评估场景与数据集](../evaluation/scenarios.md)
