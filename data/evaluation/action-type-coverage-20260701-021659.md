# 动作回路全类型设备执行覆盖记录 [EXECUTED ×4]

- 运行时间: 20260701-021659
- 设备: emulator-5554(Android 模拟器,debug build,headless）
- 数据来源: **[EXECUTED]**(四个可转发动作类型逐一在真机执行,各见终态审计事件)
- trace 文件: `data/traces/action-type-coverage-20260701-021659.jsonl`(21 行,`pm clear` 后全新落盘,逐事件可归属本轮;Git LFS pointer,独立核验需 `git lfs pull data/traces/action-type-coverage-20260701-021659.jsonl`)
- 发送通道: 验证通道取证发送器 `tests/scenarios/lib/action-forensic-sender.py`(走与生产 `AndroidAdapter` 逐字节一致的 execute 信封;非生产发送路径,见下"诚实边界")

## 为什么是"全类型"

`AndroidAdapter::classify` 会转发到设备的动作类型有四个(`NoOp` 永走本地 stub,不转发):
`KeepAlive`、`ReleaseMemory`、`PreWarmProcess`、`PrefetchFile`。早先 phase-2 只验了
`KeepAlive`;本轮把设备端 EXECUTED 覆盖扩到全部四个,每个都用与 daemon 相同的 execute
信封驱动,并核到各自处理器的终态审计事件。

## 逐类型证据

每个类型经历:execute 信封 → 设备 `BridgeExecuteProtocol.verifyExecuteEnvelope`(HMAC/freshness)
通过 → `ActionExecutorBridge.dispatch` → 各自处理器执行并落审计。设备回执 `{status:ok}` 且
socket 层记 `authorized_action_socket_execute_ok`(带 `actionType`),处理器再记各自终态事件。

| 动作类型 | target | 设备回执 summary | socket execute_ok | 处理器终态审计事件 | 终态 |
| --- | --- | --- | --- | --- | --- |
| KeepAlive | `work:collector_heartbeat` | `android_dispatched:KeepAlive` | ✅ | `keep_alive_scheduled` → `keep_alive_job_executed`(JobService 已触发) | EXECUTED |
| ReleaseMemory | `cache:prefetch` | `android_dispatched:ReleaseMemory` | ✅ | `release_memory_completed`(同步) | EXECUTED |
| PreWarmProcess | `own:warmup` | `android_dispatched:PreWarmProcess` | ✅ | `own_resources_prewarmed`(同步) | EXECUTED |
| PrefetchFile | `url:https://example.com/` | `android_dispatched:PrefetchFile` | ✅ | `prefetch_started` → `prefetch_succeeded`(异步,真下行 559 字节 text/html) | EXECUTED |

- `authorized_action_socket_execute_ok` 计数 = 4,`actionType` 分布 `{KeepAlive:1, ReleaseMemory:1, PreWarmProcess:1, PrefetchFile:1}`。
- 拒绝/失败类事件全 0:无 `*_rejected`、无 `keep_alive_failed`、无 `prefetch_failed`。
- `authorized_action_socket_empty` = 0:取证发送器发后延迟再半关写端,规避 adb forward 数据/FIN 竞态。
- `PrefetchFile` 在模拟器上真去取了 `https://example.com/`(经处理器的 https/私网地址校验),落 `prefetch_succeeded`(`bytes=559`、`contentType=text/html`),证明该类型不仅被派发,且异步下载链路也跑通。

## 脱敏闸门

trace 经采集链同一字节级脱敏闸门(覆盖 15 个敏感键)检查:`raw_title/raw_text/notification_key`
等无非空命中;`target`/`cachePath` 等路径/URL 类字段在落盘 trace 中均被脱敏为 `null`(故记录中
target 以本轮发送值列出,落盘 trace 不含其原值)。闸门干净,无脱敏回归。

## 链路与诚实边界

- **发送通道**:取证发送器走 `aios_spec::bridge` 的 execute 信封
  `{message_type, issued_at_ms, expires_at_ms, auth:{hmac_sha256}, action}`,认证标签
  HMAC-SHA256 覆盖 freshness window 与 length-prefixed `action` 字节(canonical 串
  `dipecs.android.bridge.execute.v1`),与 `AndroidAdapter::canonical_execute_envelope_input`
  / 设备侧 `BridgeExecuteProtocol.canonicalExecuteEnvelopeInput` 逐字节一致。
- **它不替代、也不修改生产发送路径**。生产是 daemon 内 `AndroidAdapter`(Rust)经内核
  loopback 直达设备内 `/system/bin/dipecsd`,字节流严格有序;取证发送器只是在"开发机 daemon
  经 adb forward 打设备 app"这条验证通道上,以发后延迟规避 adb 代理层的数据/FIN 竞态,从而
  取到"动作在真机被执行"的设备侧旁证。
- **离线对端校验**:同一组四类型也由 Rust 端到端测试
  `crates/aios-action/tests/android_bridge_e2e_test.rs::forwarded_actions_envelope_and_ok_maps_to_succeeded`
  驱动真实 `AndroidAdapter` 跑通(线信封形状 + canonical HMAC 独立重算 + 内嵌 `action_type`
  == Rust Debug 串 + 设备 `ok→Succeeded`),与本设备侧证据互为印证。

> ✅ EXECUTED ×4:`KeepAlive`、`ReleaseMemory`、`PreWarmProcess`、`PrefetchFile` 四个可转发
> 动作类型均在真机经回路执行,各见处理器终态审计事件,无拒绝/失败,trace 脱敏闸门干净。
