#!/usr/bin/env bash
# on-device-dipecsd 各阶段函数。被 tests/scenarios/on-device-dipecsd.sh source。
#
# 与 action-loop-e2e 的区别:那条链路里 dipecsd 跑在**主机**、经 adb forward 把动作
# 转发进模拟器,adb 代理层的"数据/FIN 竞态"会截断回执(旁证补偿)。本场景把交叉编译的
# dipecsd 推进**设备内**跑,直连 127.0.0.1 的 app 动作 socket —— 无 adb forward、无代理
# 失真,是最接近生产(/system/bin/dipecsd)的真机回路。

log()    { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*" | tee -a "$RUN_LOG"; }
die()    { printf '\n[FAIL] %s\n' "$*" | tee -a "$RUN_LOG" >&2; exit 1; }
banner() { printf '\n=== %s ===\n' "$*" | tee -a "$RUN_LOG"; }

# --- 纯逻辑助手(无副作用:不碰 adb/cargo、不写日志、不 die)---------------------
# 抽成独立函数,让映射与判定能被 on-device-dipecsd-selftest.sh 喂 fixture 直接断言。

# 模拟器/设备 ABI(ro.product.cpu.abi)→ rust target 三元组。空串=不支持。
emulator_abi_to_rust_target() {
  case "$1" in
    x86_64)           echo "x86_64-linux-android" ;;
    arm64-v8a)        echo "aarch64-linux-android" ;;
    x86)              echo "i686-linux-android" ;;
    armeabi-v7a)      echo "armv7-linux-androideabi" ;;
    *)                echo "" ;;
  esac
}

# rust target → NDK clang 包装器前缀(实际文件名为 <prefix><api>-clang)。
# 注意 armv7 的 NDK 前缀是 armv7a-linux-androideabi,与 rust 三元组不同。
ndk_clang_prefix_for_target() {
  case "$1" in
    x86_64-linux-android)     echo "x86_64-linux-android" ;;
    aarch64-linux-android)    echo "aarch64-linux-android" ;;
    i686-linux-android)       echo "i686-linux-android" ;;
    armv7-linux-androideabi)  echo "armv7a-linux-androideabi" ;;
    *)                        echo "" ;;
  esac
}

# rust target → cargo linker 环境变量名(大写、连字符转下划线)。
cargo_linker_env_for_target() {
  local t="$1"
  printf 'CARGO_TARGET_%s_LINKER' "$(printf '%s' "$t" | tr 'a-z-' 'A-Z_')"
}

# 端口 → /proc/net/tcp 本地地址列里的大写 hex(端口是大端十六进制)。
port_hex() { printf '%04X' "$1"; }

# 回路四态判定 —— 与 stage 日志解耦,只产状态串,可单测。
#   dispatched_ok: dipecsd 运行时 trace 里 android_dispatched 且 Succeeded 的动作数
#   app_delta:     app actions.jsonl 中 authorized_action_socket_execute_ok 的本轮增量
classify_ondevice_loop() {
  local dispatched_ok="$1" app_delta="$2"
  if [ "$dispatched_ok" -gt 0 ] && [ "$app_delta" -gt 0 ]; then
    echo "LOOP-CLOSED"              # dipecsd 派发成功 且 app 侧确认执行 —— 真机回路闭环
  elif [ "$dispatched_ok" -gt 0 ]; then
    echo "DISPATCHED-NO-APP-AUDIT"  # dipecsd 认为成功但 app 未记(可疑,需查 app 侧)
  elif [ "$app_delta" -gt 0 ]; then
    echo "APP-AUDIT-NO-DISPATCH"    # app 记了执行但 dipecsd 未标成功(不一致)
  else
    echo "NOT-DISPATCHED"           # 未闭环:无任何设备确认派发
  fi
}

# --- 副作用阶段 ----------------------------------------------------------------

# 钉定 ANDROID_SERIAL,杜绝多设备下打错设备(与 action-loop 同策略)。
pin_serial() {
  [ -n "${ANDROID_SERIAL:-}" ] && { log "沿用已指定 ANDROID_SERIAL=$ANDROID_SERIAL"; return 0; }
  local devs n
  devs="$(adb devices | awk '/\tdevice$/ {print $1}')"
  n="$(printf '%s\n' "$devs" | grep -c . || true)"
  if [ "$n" -eq 1 ]; then
    export ANDROID_SERIAL="$devs"; log "钉定 ANDROID_SERIAL=$ANDROID_SERIAL"
  elif [ "$n" -eq 0 ]; then die "无在线设备"
  else die "检测到多台设备,请显式 export ANDROID_SERIAL=<serial> 后重跑"; fi
}

stage0_preflight() {
  banner "阶段 0:环境自检"
  [ -d "$ANDROID_HOME" ] || die "ANDROID_HOME 不存在: $ANDROID_HOME"
  [ -x "$ANDROID_HOME/platform-tools/adb" ] || die "缺 adb"
  command -v cargo >/dev/null || die "缺 cargo"
  [ -f "$SAMPLE" ] || die "缺采集样本: $SAMPLE"
  pin_serial
  # 需要 root 读 app 私有 actions.jsonl(delta 验证)并从 /data/local/tmp 执行。
  # adb root 会重启 adbd;必须给超时——df 满会把模拟器 adbd 拖死,此时裸 adb root 会无限
  # 阻塞(|| true 只兜非零退出,兜不了不返回)。把"静默无限挂"降级成"30s 内带提示失败"。
  timeout 25 adb root >>"$RUN_LOG" 2>&1 || log "[warn] adb root 超时/失败,按非 root 继续"
  timeout 30 adb wait-for-device || die "adb wait-for-device 超时:设备离线/卡死,重启模拟器后重跑"
  # wait-for-device 对 offline 设备可能立即返回,补一发真实 shell 探活兜住 wedged 态。
  timeout 10 adb shell true 2>/dev/null || die "设备无响应(offline/wedged):shell 探活超时,重启模拟器后重跑"
  local ctx; ctx="$(timeout 10 adb shell id 2>/dev/null)"
  printf '%s' "$ctx" | grep -q 'uid=0' || log "[warn] adb 非 root($ctx),app 私有 trace 的 delta 验证可能取不到"
  log "环境自检通过 serial=$ANDROID_SERIAL"
}

stage1_detect_and_build() {
  banner "阶段 1:探测 ABI + 交叉编译设备内 dipecsd"
  DEVICE_ABI="$(adb shell getprop ro.product.cpu.abi 2>/dev/null | tr -d '\r')"
  DEVICE_SDK="$(adb shell getprop ro.build.version.sdk 2>/dev/null | tr -d '\r')"
  RUST_TARGET="$(emulator_abi_to_rust_target "$DEVICE_ABI")"
  [ -n "$RUST_TARGET" ] || die "不支持的设备 ABI: $DEVICE_ABI"
  log "设备 ABI=$DEVICE_ABI SDK=$DEVICE_SDK → rust target=$RUST_TARGET"

  rustup target list --installed 2>/dev/null | grep -qx "$RUST_TARGET" \
    || { log "添加 rust target $RUST_TARGET"; rustup target add "$RUST_TARGET" >>"$RUN_LOG" 2>&1 || die "rustup target add 失败"; }

  # 定位 NDK(取 $ANDROID_HOME/ndk 下版本号最大者)。
  local ndk_root; ndk_root="$(ls -d "$ANDROID_HOME/ndk"/* 2>/dev/null | sort -V | tail -1)"
  [ -n "$ndk_root" ] && [ -d "$ndk_root" ] || die "未找到 NDK(装:sdkmanager 'ndk;27.2.12479018')"
  local tc="$ndk_root/toolchains/llvm/prebuilt/linux-x86_64/bin"
  [ -d "$tc" ] || die "NDK 工具链目录缺失: $tc"

  # 选 clang 包装器:优先设备 SDK 对应 API,取不到则回退该 ABI 下可用的最高 API。
  local prefix; prefix="$(ndk_clang_prefix_for_target "$RUST_TARGET")"
  local clang="$tc/${prefix}${DEVICE_SDK}-clang"
  if [ ! -x "$clang" ]; then
    clang="$(ls "$tc/${prefix}"*-clang 2>/dev/null | grep -vE 'clang\+\+' | sort -V | tail -1)"
  fi
  [ -n "$clang" ] && [ -x "$clang" ] || die "未找到 $prefix 的 clang 包装器于 $tc"
  log "NDK=$ndk_root clang=$(basename "$clang")"

  # cc-rs(ring 等 C 依赖)+ cargo linker 都指向 NDK clang。变量名按 target 生成。
  local linker_var under
  linker_var="$(cargo_linker_env_for_target "$RUST_TARGET")"
  under="$(printf '%s' "$RUST_TARGET" | tr '-' '_')"
  export "${linker_var}=$clang"
  export "CC_${under}=$clang"
  export "AR_${under}=$tc/llvm-ar"
  export "RANLIB_${under}=$tc/llvm-ranlib"

  log "编译 dipecsd --release --target $RUST_TARGET(首次含 ring,需数十秒)..."
  cargo build -p aios-daemon --bin dipecsd --release --target "$RUST_TARGET" >>"$RUN_LOG" 2>&1 \
    || die "交叉编译失败(见 $RUN_LOG)"
  DIPECSD_BIN="target/$RUST_TARGET/release/dipecsd"
  [ -f "$DIPECSD_BIN" ] || die "产物缺失: $DIPECSD_BIN"
  log "产物:$DIPECSD_BIN ($(du -h "$DIPECSD_BIN" | cut -f1))"
}

stage2_ensure_app_socket() {
  banner "阶段 2:确认 app 已装 + 动作 socket 在监听"
  adb shell pm list packages 2>/dev/null | grep -q "$PKG" || die "app 未安装($PKG)。先跑 action-loop-e2e 或手动装 debug APK"
  adb shell pidof "$PKG" >/dev/null 2>&1 \
    || { log "app 未在跑,auto-start 拉起前台服务"; adb shell am start -n "$PKG/.MainActivity" --ez auto_start true >>"$RUN_LOG" 2>&1 || true; sleep 4; }
  local hex; hex="$(port_hex "$ACTION_PORT")"
  if adb shell 'cat /proc/net/tcp /proc/net/tcp6' 2>/dev/null | awk '{print $2}' | grep -qi ":$hex"; then
    log "动作 socket 127.0.0.1:$ACTION_PORT 在监听"
  else
    log "socket 未监听,尝试 auto-start ..."
    adb shell am start -n "$PKG/.MainActivity" --ez auto_start true >>"$RUN_LOG" 2>&1 || true
    sleep 4
    adb shell 'cat /proc/net/tcp /proc/net/tcp6' 2>/dev/null | awk '{print $2}' | grep -qi ":$hex" \
      || die "动作 socket 仍未监听,无法验证回路"
    log "动作 socket 已就绪"
  fi
}

stage3_push() {
  banner "阶段 3:推送二进制 + 采集样本进设备"
  adb push "$DIPECSD_BIN" "$DEV_BIN" >>"$RUN_LOG" 2>&1 || die "push dipecsd 失败"
  adb shell chmod 755 "$DEV_BIN" >>"$RUN_LOG" 2>&1 || die "chmod 失败"
  adb push "$SAMPLE" "$DEV_SAMPLE" >>"$RUN_LOG" 2>&1 || die "push 样本失败"
  # 冒烟:确认原生二进制能起(ABI/linker 正确),3s 后 SIGTERM 自停。
  # 注意:remote `timeout 3` 跑满会返回 124,这是预期"起来了";务必用 `|| rc=$?` 兜住,
  # 别用 `cmd; :`——`;` 分隔下 set -e 会在 cmd 非零时直接退出,根本到不了 `:`(踩过)。
  local smoke_rc=0
  adb shell "timeout 3 $DEV_BIN --no-daemon --android-trace-jsonl $DEV_SAMPLE >/dev/null 2>&1" || smoke_rc=$?
  case "$smoke_rc" in
    0|124) ;;                                              # 0=自停 124=被 timeout 杀,都说明能起
    126) die "二进制无法执行(ABI/权限不符):smoke rc=126" ;;
    127) die "设备上找不到二进制:smoke rc=127(push 失败?)" ;;
    *)   log "[warn] 冒烟异常 rc=$smoke_rc,继续(stage4 再验启动)" ;;
  esac
  log "已推送并冒烟通过:$DEV_BIN(smoke rc=$smoke_rc)"
}

stage4_run_ondevice() {
  banner "阶段 4:设备内运行 dipecsd(bridge 直连 localhost,无 adb forward)"
  # 记 app 侧执行审计基线,用增量判定本轮是否真执行。
  EXEC_OK_BEFORE="$(adb shell "grep -c authorized_action_socket_execute_ok $APP_TRACE" 2>/dev/null | tr -d '\r' || echo 0)"
  [ -n "$EXEC_OK_BEFORE" ] || EXEC_OK_BEFORE=0
  log "app execute_ok 基线=$EXEC_OK_BEFORE"

  # 窗口 10s,跑 ${RUN_SECS}s 让至少一个窗口关闭并向 bridge 转发;末窗由 shutdown flush。
  log "运行 ${RUN_SECS}s:tail $DEV_SAMPLE,bridge→127.0.0.1:$ACTION_PORT ..."
  adb shell "RUST_LOG=info \
    DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=1 \
    DIPECS_ANDROID_ACTION_BRIDGE_HOST=127.0.0.1 \
    DIPECS_ANDROID_ACTION_BRIDGE_PORT=$ACTION_PORT \
    DIPECS_ANDROID_ACTION_BRIDGE_TOKEN=$ACTION_TOKEN \
    timeout $RUN_SECS $DEV_BIN --no-daemon \
      --android-trace-jsonl $DEV_SAMPLE \
      --trace-output $DEV_RT_TRACE 2>&1" > "$DAEMON_LOG" 2>&1 || true
  log "daemon 日志:$DAEMON_LOG"
  grep -qE "processing task started|collection task started" "$DAEMON_LOG" \
    || die "dipecsd 未正常启动(见 $DAEMON_LOG)"
}

stage5_verify() {
  banner "阶段 5:验证真机回路闭环"
  # 拉运行时 trace,数「设备确认派发(android_dispatched 且 Succeeded)」的动作。
  adb shell "cat $DEV_RT_TRACE" 2>/dev/null > "$RT_TRACE_LOCAL" || true
  DISPATCHED_OK=0
  if [ -s "$RT_TRACE_LOCAL" ]; then
    DISPATCHED_OK="$(python3 - "$RT_TRACE_LOCAL" <<'PY' 2>/dev/null || echo 0
import sys, json
n = 0
for line in open(sys.argv[1], encoding="utf-8", errors="replace"):
    line = line.strip()
    if not line:
        continue
    try:
        rec = json.loads(line)
    except Exception:
        continue
    for a in rec.get("audit", []):
        out = (a.get("outcome") or {}).get("summary", "") or ""
        if a.get("terminal") == "Succeeded" and "android_dispatched" in out:
            n += 1
print(n)
PY
)"
  fi
  printf '%s' "$DISPATCHED_OK" | grep -qE '^[0-9]+$' || DISPATCHED_OK=0

  EXEC_OK_AFTER="$(adb shell "grep -c authorized_action_socket_execute_ok $APP_TRACE" 2>/dev/null | tr -d '\r' || echo 0)"
  [ -n "$EXEC_OK_AFTER" ] || EXEC_OK_AFTER=0
  APP_DELTA=$(( EXEC_OK_AFTER - EXEC_OK_BEFORE ))
  [ "$APP_DELTA" -ge 0 ] || APP_DELTA=0

  OUTCOME="$(classify_ondevice_loop "$DISPATCHED_OK" "$APP_DELTA")"
  log "设备确认派发数=$DISPATCHED_OK  app execute_ok 增量=$APP_DELTA  判定=$OUTCOME"

  # 从 daemon 日志抽一条人可读旁证。
  grep -E "device confirmed execution" "$DAEMON_LOG" | head -1 | sed 's/^/  ┗ /' | tee -a "$RUN_LOG" || true

  case "$OUTCOME" in
    LOOP-CLOSED) log "✅ 真机回路闭环:设备内 dipecsd 直发 localhost bridge,app 确认执行(无 adb forward)" ;;
    DISPATCHED-NO-APP-AUDIT) log "[warn] dipecsd 报成功但 app 侧 execute_ok 无增量(检查 app 是否 debug build / token)" ;;
    *) die "回路未闭环:判定=$OUTCOME(dispatched_ok=$DISPATCHED_OK app_delta=$APP_DELTA)" ;;
  esac
}

write_validation_record() {
  banner "写验证记录"
  DATA_SOURCE="设备内 dipecsd(交叉编译 $RUST_TARGET)+ localhost bridge,无 adb forward"
  local rec="data/evaluation/on-device-dipecsd-$(date +%Y%m%d-%H%M%S).md"
  mkdir -p data/evaluation
  {
    echo "# 设备内 dipecsd 真机回路验证"
    echo
    echo "- 时间:$(date '+%Y-%m-%d %H:%M:%S')"
    echo "- 设备:$ANDROID_SERIAL(ABI=$DEVICE_ABI SDK=$DEVICE_SDK)"
    echo "- 二进制:$DIPECSD_BIN(target=$RUST_TARGET,NDK 交叉编译)"
    echo "- 数据源:$DATA_SOURCE"
    echo "- 采集样本:$SAMPLE"
    echo "- 设备确认派发数(dipecsd 运行时 trace):$DISPATCHED_OK"
    echo "- app execute_ok 增量:$APP_DELTA(=$EXEC_OK_AFTER-$EXEC_OK_BEFORE)"
    echo "- 判定:$OUTCOME"
    echo
    echo "## 与 action-loop-e2e 的区别"
    echo
    echo "action-loop-e2e 里 dipecsd 跑在主机、经 adb forward 转发,adb 代理层的数据/FIN"
    echo "竞态会截断回执;本场景把 dipecsd 推进设备内直连 127.0.0.1 的 app 动作 socket,"
    echo "无 adb forward、无代理失真,是最接近生产(/system/bin/dipecsd)的真机回路。"
  } > "$rec"
  log "验证记录:$rec"
  VALIDATION_RECORD="$rec"
}
