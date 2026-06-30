#!/usr/bin/env bash
# emulator-e2e 各阶段函数。被 tests/scenarios/emulator-e2e.sh source。

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
  # 幂等但不能复用过期 APK:只要 app/src 或 gradle 配置里有任何文件比 APK 新,
  # 就强制重编译。否则会装上陈旧 APK,采集/脱敏逻辑与当前源码不一致 —— 这正是
  # 一次真实事故的根因(旧 APK 把通知原文写进 trace,绕过了源码里的脱敏)。
  local stale=""
  if [ -f "$APK" ]; then
    stale="$(find apps/android-collector/app/src apps/android-collector/app/build.gradle* \
      apps/android-collector/build.gradle* apps/android-collector/gradle.properties \
      -type f -newer "$APK" 2>/dev/null | head -1)"
  fi
  if [ ! -f "$APK" ] || [ -n "$stale" ]; then
    [ -n "$stale" ] && log "检测到源码比 APK 新(如 $stale),强制重编译"
    log "编译 debug APK ..."
    (cd apps/android-collector && ./gradlew :app:assembleDebug) >>"$RUN_LOG" 2>&1 \
      || die "APK 编译失败"
  else
    log "APK 不比源码旧,复用"
  fi
  log "安装 APK ..."
  adb install -r -g "$APK" >>"$RUN_LOG" 2>&1 || die "APK 安装失败"
  log "已安装 $PKG"
}

NOTIF_SVC="$PKG/.services.NotificationCollectorService"

stage4_grant_and_start() {
  banner "阶段 4:授权 + 启动采集"
  # 先清 app 数据,杜绝跨轮旧 trace 累积。adb install -r 会保留 app 私有目录,
  # 若不清,上一轮(甚至旧 APK)写的 actions.jsonl 会留存,阶段 6 run-as 拉出的
  # 是历史累积而非本轮采集 —— 一次真实事故里正是它让旧 APK 的未脱敏原文混进结果、
  # 被误判为本轮 REAL。pm clear 同时会清掉权限,故必须在下面重新授权之前执行。
  adb shell pm clear "$PKG" >>"$RUN_LOG" 2>&1 || log "[warn] pm clear 失败,trace 可能含上轮残留"
  # Usage Access(appops)
  adb shell appops set "$PKG" GET_USAGE_STATS allow >>"$RUN_LOG" 2>&1 || die "授 Usage 失败"
  # POST_NOTIFICATIONS(运行时权限,Android 13+)
  adb shell pm grant "$PKG" android.permission.POST_NOTIFICATIONS >>"$RUN_LOG" 2>&1 || true
  # NotificationListener:加进 enabled 列表
  adb shell cmd notification allow_listener "$NOTIF_SVC" >>"$RUN_LOG" 2>&1 || \
    log "[warn] allow_listener 失败,通知源可能采不到"
  # 启动前台采集服务。
  # 注意:CollectorForegroundService 在 manifest 里是 exported=false,Android uid 模型
  # 禁止 adb shell(uid 2000)直接 am start[-foreground]-service 它(报
  # "Requires permission not exported from uid ...")。这是平台限制,任何真机都一样,
  # 不是 app 缺陷。真正的 rawEvent 源是 NotificationListenerService(exported=true,
  # 已在上面 allow_listener 启用并会自动 connect 采集);前台服务只是 UsageStats 轮询
  # 等的加成,采不到也不影响通知源落盘。故这里 best-effort:能起就起,起不了只告警,
  # 不让装备类 die 掉,后续阶段 6 仍能从私有目录 run-as 拉到真实脱敏 trace。
  adb shell am start-foreground-service -n "$PKG/.services.CollectorForegroundService" \
    -a com.dipecs.collector.action.START >>"$RUN_LOG" 2>&1 || \
    log "[warn] 前台采集服务无法经 adb 启动(exported=false,平台限制);通知监听源仍在采集"
  sleep 3
  log "权限已授,采集已就绪(通知监听源已连接)"
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

  # 脱敏闸门:trace 进 git 的前提是写盘即脱敏(EventStore.sanitizeForTrace 把
  # raw_title/raw_text/notification_key 清空)。若拉出的 trace 仍含非空敏感原文,
  # 说明装的不是当前源码编译的 APK(或脱敏回归),这是会泄漏隐私的严重事故 ——
  # 立即停下,绝不让原文进 replay 或 git。宁可整轮失败也不冒充"已脱敏"。
  if [ -s "$trace" ]; then
    local leak
    # LC_ALL=C + grep -a:把输入当字节流。否则 UTF-8 locale 下 [^"]+ 对含多字节字符
    # (如省略号 …)的原文行可能匹配失败,导致泄漏漏检 —— 闸门必须按字节扫,不留死角。
    leak="$(LC_ALL=C grep -aoE '"(raw_title|raw_text|notification_key)":"[^"]+"' "$trace" 2>/dev/null \
      | LC_ALL=C grep -av '":""' | head -3 || true)"
    [ -n "$leak" ] && die "脱敏闸门拦截:trace 含未脱敏原文(疑似旧 APK 或脱敏回归):$leak"
  fi

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

write_validation_record() {
  banner "写验证记录"
  local rec="data/evaluation/emulator-e2e-$TS.md"
  local tag="$DATA_SOURCE"
  [ "$DATA_SOURCE" = "REAL" ] && [ "$MODE" = "auto" ] && \
    grep -q '"NotificationPosted"' "$TRACE_FILE" 2>/dev/null || \
    { [ "$DATA_SOURCE" = "REAL" ] && tag="REAL(部分源)"; }
  {
    echo "# 模拟器端到端验证记录 [$DATA_SOURCE]"
    echo
    echo "- 运行时间: $TS"
    echo "- 模式: $MODE"
    echo "- 数据来源: **[$tag]**"
    echo "- AVD: $AVD_NAME  系统镜像: android-35;google_apis;x86_64"
    echo "- rawEvent 行数: $RAW_ROWS"
    echo "- trace 文件: $TRACE_FILE"
    echo "- 审计哈希: \`${AUDIT_HASH:-未捕获}\`  (数据源: $DATA_SOURCE)"
    echo
    if [ "$DATA_SOURCE" = "FALLBACK" ]; then
      echo "> ⚠ FALLBACK:本次未从模拟器真实采集,数据为预置样本"
      echo "> \`$SAMPLE\`,非本次运行产物。"
    fi
  } > "$rec"
  log "验证记录写入 $rec"
}
