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
