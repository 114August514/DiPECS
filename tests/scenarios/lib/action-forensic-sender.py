#!/usr/bin/env python3
"""动作回路验证通道取证发送器(仅验证用,非生产发送路径)。

为什么需要它:生产发送链是 `aios-action::AndroidAdapter`(Rust),daemon 经它把
KeepAlive 转发到设备 app 的 localhost socket。但在"开发机 daemon → adb forward →
设备 app"这条**验证通道**上,adb 的用户态 TCP 代理在转发数据与转发连接关闭(FIN)
之间存在调度间隙:发送端 write 后立即关连接,FIN 会追上尚未推送到设备的数据,设备
侧 app `accept` 后第一次 read 即得 EOF,读到空 payload,记
`authorized_action_socket_empty`,动作丢失。这是 adb forward 的固有失真(业界亦有
记录:sair PR#6 "Fix ADB proxy TCP write fragmentation"、StackOverflow
"adb forward connect successfully but transfer empty data")。

生产部署无此问题:daemon 终态是设备内 `/system/bin/dipecsd`(见
docs/src/design/daemon-architecture.md),与 app 同机走内核 loopback,字节流严格
有序,数据必先于 FIN 到达。故不为这条测试通道在生产 Rust 里塞延迟/旋钮。

本发送器走**与生产 daemon(`AndroidAdapter`)相同的线协议**——`aios_spec::bridge`
的 execute 信封 `{message_type:"execute", issued_at_ms, expires_at_ms, auth:{hmac_sha256},
action:"<AuthorizedAction JSON 字符串>"}`。认证标签 HMAC-SHA256 覆盖 freshness window
与 length-prefixed action 字节(canonical 串 `dipecs.android.bridge.execute.v1`),与
`AndroidAdapter::canonical_execute_envelope_input` / 设备侧
`BridgeExecuteProtocol.canonicalExecuteEnvelopeInput` 逐字节一致。取证器仅在 write 后
延迟再 shutdown,给 adb 推送数据的时间——以此在验证通道上取到"动作被真实执行"的旁证
(Android 落 keep_alive_job_executed)。

诚实边界:这是验证通道取证,证明的是"信封格式/HMAC/freshness/Android 校验/执行链"
成立;它不替代、也不修改生产发送路径。daemon 真发那一轨的真实表现(大概率 empty)由
脚本如实记录,不被本旁证掩盖。

用法: action-forensic-sender.py <host> <port> <token> [delay_sec] [action_type] [target] [urgency]
  默认 action_type=KeepAlive target=work:collector_heartbeat urgency=Immediate(向后兼容原 KeepAlive 调用)。
  其余可转发类型按 AndroidAdapter::classify / ActionExecutorBridge.dispatch 取各自合法 target:
    ReleaseMemory  cache:prefetch     (CacheTrimmer → release_memory_completed)
    PreWarmProcess own:warmup         (OwnResourceWarmer → own_resources_prewarmed)
    PrefetchFile   url:https://…      (AccessibleContentPrefetcher → prefetch_started → succeeded/failed,需网络)
退出码: 0=已发送(payload 已写出并延迟后关闭) 非0=连接/发送失败
"""
import hashlib
import hmac
import json
import os
import socket
import sys
import time

# 与 Rust AndroidAdapter 的 ANDROID_ACTION_PAYLOAD_TTL_MS 一致(envelope freshness 窗口)。
PAYLOAD_TTL_MS = 60_000


def build_action_json(authorized_at_ms, action_type, target, urgency):
    """构造与 `aios_core::governance::AuthorizedAction` 同形的 action JSON 字符串。

    字段形状参照 AuthorizedAction 的 Serialize:intent_id / coord /
    action{action_type,target,urgency} / effect / authorized_at_ms。这段字符串的
    **字节**会进入 canonical HMAC 输入;设备按收到的同一段字节重算校验。
    """
    target_value = target if target else None
    return json.dumps({
        "intent_id": "action-loop-forensic",
        "coord": {"window_ordinal": 0, "intent_ordinal": 0, "action_ordinal": 0},
        "action": {"action_type": action_type, "target": target_value, "urgency": urgency},
        "effect": "LocalStateChange",
        "authorized_at_ms": authorized_at_ms,
    })


def canonical_execute_envelope_input(issued_at_ms, expires_at_ms, action_json):
    """逐字节复刻 Rust `canonical_execute_envelope_input` / Kotlin
    `BridgeExecuteProtocol.canonicalExecuteEnvelopeInput`。

    长度前缀用 action_json 的 **UTF-8 字节数**(Rust `String::len()` /
    Kotlin `toByteArray(UTF_8).size`),非字符数。
    """
    action_bytes_len = len(action_json.encode("utf-8"))
    return (
        "dipecs.android.bridge.execute.v1\n"
        f"issued_at_ms:{issued_at_ms}\n"
        f"expires_at_ms:{expires_at_ms}\n"
        f"action:{action_bytes_len}:{action_json}"
    )


def main():
    if len(sys.argv) < 4:
        print("usage: action-forensic-sender.py <host> <port> <token> [delay_sec] "
              "[action_type] [target] [urgency]",
              file=sys.stderr)
        return 2
    host = sys.argv[1]
    port = int(sys.argv[2])
    token = sys.argv[3]
    delay = float(sys.argv[4]) if len(sys.argv) > 4 else 1.5

    # The Android bridge checks issued/expires against the device wall clock
    # (AuthorizedActionSocketServer.kt: now = System.currentTimeMillis()) with
    # only a 30s skew allowance. Some real devices have a stale system clock, so
    # host-clock envelopes can be rejected as expired. DEVICE_CLOCK_OFFSET_MS is
    # device clock minus host clock in milliseconds, measured by collection
    # scripts when needed. The default 0 keeps the original behavior.
    clock_offset_ms = int(os.environ.get("DEVICE_CLOCK_OFFSET_MS", "0"))
    issued_at_ms = int(time.time() * 1000) + clock_offset_ms
    expires_at_ms = issued_at_ms + PAYLOAD_TTL_MS
    # 可选位置参数:类型/目标/紧迫度。默认 KeepAlive 心跳,保持原调用向后兼容。
    # 设备 dispatch 只读 action_type+target;urgency 仅随 action 字节进 canonical HMAC,
    # 取何值都被设备按收到的同一段字节重算校验。
    action_type = sys.argv[5] if len(sys.argv) > 5 else "KeepAlive"
    target = sys.argv[6] if len(sys.argv) > 6 else "work:collector_heartbeat"
    urgency = sys.argv[7] if len(sys.argv) > 7 else "Immediate"

    action_json = build_action_json(issued_at_ms, action_type, target, urgency)
    canonical = canonical_execute_envelope_input(issued_at_ms, expires_at_ms, action_json)
    tag = hmac.new(token.encode(), canonical.encode("utf-8"), hashlib.sha256).hexdigest()

    # execute 信封:message_type + issued/expires + auth.hmac_sha256 + action(字符串)。
    payload = json.dumps({
        "message_type": "execute",
        "issued_at_ms": issued_at_ms,
        "expires_at_ms": expires_at_ms,
        "auth": {"hmac_sha256": tag},
        "action": action_json,
    })

    try:
        s = socket.create_connection((host, port), timeout=8)
    except OSError as err:
        print(f"connect {host}:{port} failed: {err}", file=sys.stderr)
        return 1
    try:
        s.sendall(payload.encode())
        # 关键:发后延迟再半关写端,给 adb forward 把数据推到设备的时间,规避
        # 数据/FIN 竞态。生产 loopback 无需此延迟。
        time.sleep(delay)
        s.shutdown(socket.SHUT_WR)
        s.settimeout(2.0)
        # 设备 responder 回送 {status:ok/rejected/error};读到即作旁证记录,读不到
        # (EOF/超时)也不改变"payload 已发出"的结论,故 best-effort。
        verdict = ""
        try:
            raw = s.recv(512)
            if raw:
                verdict = f", device={raw.decode('utf-8', 'replace').strip()}"
        except OSError:
            pass
    finally:
        s.close()
    print(f"forensic payload sent ({len(payload)} bytes, action={action_type} target={target}, "
          f"hmac={tag[:12]}..., delay={delay}s{verdict})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
