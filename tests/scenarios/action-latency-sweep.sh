#!/usr/bin/env bash
# 动作执行延迟 sweep:在真实 Android 设备/模拟器上测量四类动作从发端到设备确认的耗时。
#
# 这是 UX 价值的一个代理指标:动作被确认并派发得越快,对用户可见行为的干预窗口越小。
# 运行需要:
#   - 已启动的 Android 模拟器或真机(通过 ANDROID_SERIAL 指定)
#   - 已安装 com.dipecs.collector debug APK
#   - adb forward tcp:46321 tcp:46321

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export PATH="$PATH:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator"

PKG="com.dipecs.collector"
ACTION_PORT=46321
TOKEN="dipecs-dev-emulator-shared-token-00000000"
SENDER="tests/scenarios/lib/action-forensic-sender.py"
DELAY=1.5

die() { echo "[error] $*" >&2; exit 1; }

pin_serial() {
  if [ -n "${ANDROID_SERIAL:-}" ]; then
    echo "ANDROID_SERIAL=$ANDROID_SERIAL"; return 0
  fi
  local devs n
  devs="$(adb devices | awk '/\tdevice$/ {print $1}')"
  n="$(printf '%s\n' "$devs" | grep -c . || true)"
  if [ "$n" -eq 1 ]; then
    export ANDROID_SERIAL="$devs"
    echo "钉定 ANDROID_SERIAL=$ANDROID_SERIAL"
  elif [ "$n" -eq 0 ]; then
    die "无在线设备;请先启动模拟器或连接真机"
  else
    die "多台设备,请 export ANDROID_SERIAL=<serial>"
  fi
}

ensure_app_and_forward() {
  if ! adb shell pidof "$PKG" >/dev/null 2>&1; then
    echo "=== 拉起 app 前台服务 ==="
    adb shell am start -n "$PKG/.MainActivity" --ez auto_start true >/dev/null 2>&1 || true
    sleep 4
  fi
  adb shell pidof "$PKG" >/dev/null 2>&1 || die "$PKG 未运行"
  adb forward tcp:$ACTION_PORT tcp:$ACTION_PORT >/dev/null 2>&1 || true
}

# 发送一次取证动作,解析设备回执里的 latency_us。
measure_action() {
  local atype="$1" target="$2"
  local tmp
  tmp="$(mktemp)"
  python3 "$SENDER" 127.0.0.1 "$ACTION_PORT" "$TOKEN" "$DELAY" "$atype" "$target" Immediate >"$tmp" 2>&1 || true
  local line
  line="$(cat "$tmp")"
  rm -f "$tmp"

  local device_us
  device_us="$(printf '%s' "$line" | python3 -c '
import sys, json, re
try:
    text = sys.stdin.read()
    m = re.search(r"device=({.*?})", text)
    if not m:
        print("NA")
        sys.exit(0)
    data = json.loads(m.group(1))
    v = data.get("latency_us")
    print(int(v) if isinstance(v, (int, float)) and v is not None else "NA")
except Exception:
    print("NA")
' 2>/dev/null || echo NA)"

  if [ "$device_us" = "NA" ]; then
    na_count=$((na_count + 1))
  fi
  printf '%s\t%s\t%s\n' "$atype" "$target" "$device_us"
}

main() {
  pin_serial
  ensure_app_and_forward

  echo "=== 动作设备侧确认延迟 sweep ==="
  printf '%s\t%s\t%s\n' "action_type" "target" "device_latency_us"

  local na_count=0

  measure_action KeepAlive "work:collector_heartbeat"
  measure_action ReleaseMemory "cache:prefetch"
  measure_action PreWarmProcess "own:warmup"
  measure_action PrefetchFile "url:https://example.com/"

  echo
  if [ "$na_count" -gt 0 ]; then
    echo "警告:$na_count 次测量未能解析 latency_us,请检查设备回执格式。" >&2
  fi
  echo "说明:device_latency_us 来自设备 AuthorizedActionSocketServer 回执中的 latency_us,"
  echo "表示设备从 accept 到 dispatch 完成并发出 {status:ok} 的耗时。"
  echo "KeepAlive/ReleaseMemory/PreWarmProcess 是同步或近同步完成;PrefetchFile 是异步派发,"
  echo "回执只代表已入队。"
}

main "$@"
