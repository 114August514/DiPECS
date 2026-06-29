#!/usr/bin/env bash
# emulator-e2e 各阶段函数。被 scripts/emulator-e2e.sh source。

log()    { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*" | tee -a "$RUN_LOG"; }
die()    { printf '\n[FAIL] %s\n' "$*" | tee -a "$RUN_LOG" >&2; exit 1; }
banner() { printf '\n=== %s ===\n' "$*" | tee -a "$RUN_LOG"; }

stage0_preflight() {
  banner "阶段 0:环境自检"
  [ -d "$ANDROID_HOME" ] || die "ANDROID_HOME 不存在: $ANDROID_HOME"
  command -v java >/dev/null || die "缺 java"
  [ -x "$ANDROID_HOME/platform-tools/adb" ] || die "缺 adb"
  [ -x "$ANDROID_HOME/emulator/emulator" ] || die "缺 emulator"
  [ -x "$REPO_ROOT/apps/android-collector/gradlew" ] || die "缺 gradlew"
  log "环境自检通过"
}

SYS_IMG="system-images;android-35;google_apis;x86_64"

stage1_provision_sdk() {
  banner "阶段 1:配齐 SDK(幂等)"
  if [ ! -x "$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" ]; then
    die "缺 cmdline-tools。请先手动安装到 \$ANDROID_HOME/cmdline-tools/latest(见 README),或运行 sdkmanager 自举"
  fi
  if [ ! -d "$ANDROID_HOME/system-images/android-35" ]; then
    log "下载 system-image: $SYS_IMG ..."
    yes | sdkmanager "$SYS_IMG" >>"$RUN_LOG" 2>&1 || die "system-image 下载失败(阶段 1 停)"
  else
    log "system-image 已存在,复用"
  fi
  if ! avdmanager list avd 2>/dev/null | grep -q "Name: $AVD_NAME"; then
    log "创建 AVD: $AVD_NAME"
    echo no | avdmanager create avd -n "$AVD_NAME" -k "$SYS_IMG" --force >>"$RUN_LOG" 2>&1 \
      || die "AVD 创建失败"
  else
    log "AVD $AVD_NAME 已存在,复用"
  fi
}

stage2_boot_emulator() {
  banner "阶段 2:起模拟器"
  if adb devices | grep -q "emulator-.*device"; then
    log "已有模拟器在线,复用"; return 0
  fi
  log "后台启动模拟器 $AVD_NAME ..."
  "$ANDROID_HOME/emulator/emulator" -avd "$AVD_NAME" \
    -no-window -no-audio -no-snapshot -gpu swiftshader_indirect \
    >>"$RUN_LOG" 2>&1 &
  adb wait-for-device
  local t=0
  until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do
    sleep 2; t=$((t+2)); [ "$t" -ge 180 ] && die "模拟器启动超时(180s)"
  done
  log "模拟器开机完成(${t}s)"
}

APK="apps/android-collector/app/build/outputs/apk/debug/app-debug.apk"

stage3_build_install() {
  banner "阶段 3:编译 + 安装"
  if [ ! -f "$APK" ]; then
    log "编译 debug APK ..."
    (cd apps/android-collector && ./gradlew :app:assembleDebug) >>"$RUN_LOG" 2>&1 \
      || die "APK 编译失败"
  else
    log "APK 已存在,复用(如需重编译删除 $APK)"
  fi
  log "安装 APK ..."
  adb install -r -g "$APK" >>"$RUN_LOG" 2>&1 || die "APK 安装失败"
  log "已安装 $PKG"
}

NOTIF_SVC="$PKG/.services.NotificationCollectorService"

stage4_grant_and_start() {
  banner "阶段 4:授权 + 启动采集"
  # Usage Access(appops)
  adb shell appops set "$PKG" GET_USAGE_STATS allow >>"$RUN_LOG" 2>&1 || die "授 Usage 失败"
  # POST_NOTIFICATIONS(运行时权限,Android 13+)
  adb shell pm grant "$PKG" android.permission.POST_NOTIFICATIONS >>"$RUN_LOG" 2>&1 || true
  # NotificationListener:加进 enabled 列表
  adb shell cmd notification allow_listener "$NOTIF_SVC" >>"$RUN_LOG" 2>&1 || \
    log "[warn] allow_listener 失败,通知源可能采不到"
  # 启动前台采集服务
  adb shell am start-foreground-service -n "$PKG/.services.CollectorForegroundService" \
    -a com.dipecs.collector.action.START >>"$RUN_LOG" 2>&1 || die "启动采集服务失败"
  sleep 3
  log "权限已授,采集服务已启动"
}

stage5_generate_events() {
  banner "阶段 5:制造事件(mode=$MODE)"
  if [ "$MODE" = "manual" ]; then
    printf '\n>>> 环境已就绪。请在模拟器里操作:打开几个应用、触发几条通知。\n'
    printf '>>> 完成后按回车继续……\n'
    read -r _
  else
    # auto:切应用(AppTransition)+ 发通知(可能采不到,后续判定)
    adb shell am start -n com.android.settings/.Settings >>"$RUN_LOG" 2>&1 || true
    sleep 2
    adb shell am start -a android.intent.action.VIEW -d "https://example.com" >>"$RUN_LOG" 2>&1 || true
    sleep 2
    adb shell cmd notification post -S bigtext -t 'e2e-test' tag-e2e 'hello from e2e' >>"$RUN_LOG" 2>&1 || true
    sleep 3
  fi
  log "事件制造完成,等待 app 写盘"
  sleep 3
}

SAMPLE="data/traces/android_real_device_sample.redacted.jsonl"

stage6_pull_and_replay() {
  banner "阶段 6:取数据 + 回放"
  local trace="data/traces/emulator-e2e-$TS.jsonl"
  # run-as 拉已脱敏 trace(debug build 可 run-as)
  adb shell run-as "$PKG" cat files/traces/actions.jsonl > "$trace" 2>>"$RUN_LOG" || true
  local raw_rows=0
  [ -s "$trace" ] && raw_rows="$(grep -c '"rawEvent"' "$trace" 2>/dev/null || echo 0)"
  log "采集到 rawEvent 行数: $raw_rows"

  if [ "$raw_rows" -gt 0 ]; then
    DATA_SOURCE="REAL"
    RAW_ROWS="$raw_rows"
  else
    banner "[FALLBACK] 本次未从模拟器真实采集,改用预置样本"
    cp "$SAMPLE" "data/traces/emulator-e2e-$TS.FALLBACK.jsonl"
    trace="data/traces/emulator-e2e-$TS.FALLBACK.jsonl"
    DATA_SOURCE="FALLBACK"
    RAW_ROWS=0
  fi
  TRACE_FILE="$trace"

  local ndjson="data/evaluation/emulator-e2e-$TS.ndjson"
  local auditlog="data/evaluation/emulator-e2e-$TS.audit"
  log "运行 aios-cli replay ..."
  cargo run -q -p aios-cli -- replay "$trace" --output "$ndjson" --audit "$auditlog" \
    >>"$RUN_LOG" 2>&1 || die "replay 失败(replay 是装备类,失败即停)"
  # 完整 audit_hash 只稳定出现在 NDJSON summary 里(stderr 日志带 ANSI 转义会割裂 sha256: 前缀);
  # 保留 sha256: 前缀,与 golden test 钉死的格式一致。
  AUDIT_HASH="$(grep -oE 'sha256:[0-9a-f]{64}' "$ndjson" 2>/dev/null | tail -1)"
  log "replay 完成 audit_hash=${AUDIT_HASH:-未捕获} 数据源=$DATA_SOURCE"
}
