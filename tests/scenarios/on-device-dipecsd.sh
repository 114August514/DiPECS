#!/usr/bin/env bash
# 把交叉编译的 dipecsd 推进模拟器/真机里跑,直连设备内 app 动作 socket 验证回路闭环。
#
# 与 action-loop-e2e 互补:那条是 dipecsd 跑主机 + adb forward(有代理层 FIN 竞态失真);
# 本条是 dipecsd 跑设备内 + localhost 直连(无 adb forward),最接近生产 /system/bin/dipecsd。
#
# 用法: tests/scenarios/on-device-dipecsd.sh
# 先决: 一台在线模拟器/设备(userdebug,adb root 可用)、装了 debug 版 com.dipecs.collector、
#        NDK(sdkmanager 'ndk;27.2.12479018')、对应 rust android target。
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"
SCENARIO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# 选一个真正可用的 SDK 根:优先「既有 adb 又有 ndk」的(避免 ambient ANDROID_HOME
# 指向某个缺 adb 或缺 NDK 的半份 SDK),否则退回任一有 adb 的,最后默认 ~/Android/Sdk。
_pick_sdk() {
  local want_ndk="$1"; shift
  local c
  for c in "$@"; do
    [ -n "$c" ] && [ -x "$c/platform-tools/adb" ] || continue
    [ "$want_ndk" = 1 ] && ! ls -d "$c/ndk"/* >/dev/null 2>&1 && continue
    printf '%s' "$c"; return 0
  done
  return 1
}
_CANDS=("${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}" "$HOME/Android/Sdk" "/opt/android-sdk")
ANDROID_HOME="$(_pick_sdk 1 "${_CANDS[@]}" || _pick_sdk 0 "${_CANDS[@]}" || printf '%s' "$HOME/Android/Sdk")"
export ANDROID_HOME
export PATH="$PATH:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$ANDROID_HOME/cmdline-tools/latest/bin"

TS="$(date +%Y%m%d-%H%M%S)"
mkdir -p logs data/evaluation
RUN_LOG="logs/on-device-dipecsd-$TS.log"
DAEMON_LOG="logs/on-device-dipecsd-daemon-$TS.log"
RT_TRACE_LOCAL="logs/on-device-dipecsd-rt-$TS.ndjson"

PKG="com.dipecs.collector"
ACTION_PORT="${ACTION_PORT:-46321}"
# debug build 的固定开发 token(CollectorPreferences.DEBUG_ACTION_SOCKET_TOKEN);
# 可被 ACTION_TOKEN env 覆盖(如设备设过 debug.dipecs.token)。
ACTION_TOKEN="${ACTION_TOKEN:-dipecs-dev-emulator-shared-token-00000000}"
SAMPLE="${SAMPLE:-data/traces/android_real_device_sample.redacted.jsonl}"
RUN_SECS="${RUN_SECS:-14}"

DEV_BIN="/data/local/tmp/dipecsd"
DEV_SAMPLE="/data/local/tmp/dipecs-sample.jsonl"
DEV_RT_TRACE="/data/local/tmp/dipecs-rt.ndjson"
APP_TRACE="/data/data/$PKG/files/traces/actions.jsonl"

source "$SCENARIO_DIR/lib/on-device-dipecsd-stages.sh"
log "on-device-dipecsd 启动 ts=$TS"

stage0_preflight
stage1_detect_and_build
stage2_ensure_app_socket
stage3_push
stage4_run_ondevice
stage5_verify
write_validation_record
banner "完成 判定=$OUTCOME 数据源=$DATA_SOURCE 记录=$VALIDATION_RECORD"
