#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
ADB="${ADB:-$ANDROID_HOME/platform-tools/adb}"
APK="${APK:-apps/android-collector/app/build/outputs/apk/debug/app-debug.apk}"
PKG="${PKG:-com.dipecs.collector}"
OUT_DIR="${OUT_DIR:-data/evaluation/real-device-validation}"
TS="$(date +%Y%m%d-%H%M%S)"

if [ ! -x "$ADB" ]; then
  echo "adb not found: $ADB" >&2
  exit 1
fi

if [ ! -f "$APK" ]; then
  echo "APK not found: $APK" >&2
  echo "Build it first: cd apps/android-collector && ./gradlew assembleDebug" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"

echo "[1/5] device"
"$ADB" devices

echo "[2/5] install APK"
"$ADB" install -r "$APK"

echo "[3/5] launch validation UI"
"$ADB" shell am start -n "$PKG/.DeviceValidationActivity" >/dev/null

echo "[4/5] waiting for manual run"
echo "在手机上勾选测试并点击“开始验证”。完成后按 Enter 拉取结果。"
read -r _

echo "[5/5] pull validation artifacts"
"$ADB" pull "/sdcard/Android/data/$PKG/files/validation" "$OUT_DIR/$TS-validation" || true
"$ADB" pull "/sdcard/Android/data/$PKG/files/performance" "$OUT_DIR/$TS-performance" || true
"$ADB" pull "/sdcard/Android/data/$PKG/files/traces/actions.jsonl" "$OUT_DIR/$TS-actions.jsonl" || true

if [ -f "$OUT_DIR/$TS-actions.jsonl" ]; then
  echo "[6/6] compute local next-app accuracy"
  CSV="$OUT_DIR/$TS-next-app.csv"
  ARTIFACT="$OUT_DIR/$TS-next-app-artifact.json"
  REPORT="$OUT_DIR/$TS-next-app-report.json"
  python3 tools/evaluate/android_trace_to_lsapp.py \
    --input "$OUT_DIR/$TS-actions.jsonl" \
    --output "$CSV"
  if [ "$(wc -l < "$CSV")" -ge 31 ]; then
    cargo run -p aios-cli -- train-next-app \
      --input "$CSV" \
      --output "$ARTIFACT" \
      --history-len 5 \
      --horizon-secs 300
    cargo run -p aios-cli -- eval-next-app \
      --input "$CSV" \
      --artifact "$ARTIFACT" \
      --output "$REPORT" \
      --history-len 5 \
      --horizon-secs 300
    echo "accuracy report: $REPORT"
  else
    echo "not enough app transitions for accuracy; collect at least 30 foreground switches"
  fi
fi

echo "done: $OUT_DIR/$TS-validation"
