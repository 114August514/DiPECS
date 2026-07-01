#!/usr/bin/env bash
# on-device-dipecsd 纯逻辑自测:不依赖 adb / 模拟器 / cargo / NDK,只对 stage 里抽出的
# 纯函数(ABI→target 映射、clang 前缀、linker 变量名、端口 hex、回路四态判定)喂 fixture 断言。
# 跑法:bash tests/scenarios/lib/on-device-dipecsd-selftest.sh  (退出码 0=全过)
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN_LOG="/dev/null"
# shellcheck source=/dev/null
source "$SELF_DIR/on-device-dipecsd-stages.sh"

PASS=0
FAIL=0
ok()  { PASS=$((PASS + 1)); printf '  ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL %s\n         expected=[%s]\n         actual  =[%s]\n' "$1" "$2" "$3"; }
eq()  { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "$2" "$3"; fi; }

echo "== emulator_abi_to_rust_target =="
eq "x86_64"       "x86_64-linux-android"      "$(emulator_abi_to_rust_target x86_64)"
eq "arm64-v8a"    "aarch64-linux-android"     "$(emulator_abi_to_rust_target arm64-v8a)"
eq "x86"          "i686-linux-android"        "$(emulator_abi_to_rust_target x86)"
eq "armeabi-v7a"  "armv7-linux-androideabi"   "$(emulator_abi_to_rust_target armeabi-v7a)"
eq "unknown→空"   ""                          "$(emulator_abi_to_rust_target riscv64)"

echo "== ndk_clang_prefix_for_target(armv7 前缀与 rust 三元组不同,是主要陷阱)=="
eq "x86_64"  "x86_64-linux-android"       "$(ndk_clang_prefix_for_target x86_64-linux-android)"
eq "aarch64" "aarch64-linux-android"      "$(ndk_clang_prefix_for_target aarch64-linux-android)"
eq "armv7"   "armv7a-linux-androideabi"   "$(ndk_clang_prefix_for_target armv7-linux-androideabi)"

echo "== cargo_linker_env_for_target =="
eq "x86_64"  "CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER"  "$(cargo_linker_env_for_target x86_64-linux-android)"
eq "aarch64" "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER" "$(cargo_linker_env_for_target aarch64-linux-android)"

echo "== port_hex(/proc/net/tcp 端口大端 hex)=="
eq "46321→B4F1" "B4F1" "$(port_hex 46321)"
eq "8080→1F90"  "1F90" "$(port_hex 8080)"

echo "== classify_ondevice_loop(回路四态)=="
eq "派发成功+app确认 → 闭环"     "LOOP-CLOSED"             "$(classify_ondevice_loop 1 1)"
eq "派发成功+app无增量 → 可疑"   "DISPATCHED-NO-APP-AUDIT" "$(classify_ondevice_loop 2 0)"
eq "无派发+app有增量 → 不一致"   "APP-AUDIT-NO-DISPATCH"   "$(classify_ondevice_loop 0 1)"
eq "都为0 → 未闭环"              "NOT-DISPATCHED"          "$(classify_ondevice_loop 0 0)"

echo
printf '结果:%d 过 / %d 败\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
