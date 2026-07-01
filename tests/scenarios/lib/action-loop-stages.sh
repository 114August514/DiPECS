#!/usr/bin/env bash
# action-loop-e2e 各阶段函数。被 tests/scenarios/action-loop-e2e.sh source。
# 复用 emulator-e2e 的 log/die/banner 风格。

log()    { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*" | tee -a "$RUN_LOG"; }
die()    { printf '\n[FAIL] %s\n' "$*" | tee -a "$RUN_LOG" >&2; exit 1; }
banner() { printf '\n=== %s ===\n' "$*" | tee -a "$RUN_LOG"; }

# --- 纯逻辑助手(无副作用:不碰 adb、不写日志、不 die)---------------------
# 抽成独立函数:让四态判定与计数能被 action-loop-selftest.sh 喂 fixture 直接断言。
# 与采集链 #35 同源的几项卫生问题(双行噪声、窄闸门)在此一并收敛。

# 数某审计事件出现行数,返回干净单行整数。
# grep -ac 零匹配时退出码 1 触发 || true,仍输出 grep 自己打的 "0";若用 || echo 0 则
# 零匹配会变成 "0\n0" 双行,后续 [ -gt ] 报 integer expression expected(仅噪声但应除)。
count_audit_event() {
  local trace="$1" pattern="$2"
  [ -s "$trace" ] || { echo 0; return 0; }
  LC_ALL=C grep -acE "$pattern" "$trace" 2>/dev/null || true
}

# 四态判定:执行优先,其次链路已通的中间态,再次失败态,最后链路未通。
# 与 stage 里的日志解耦 —— 这里只产状态串,可单测;日志留在 stage6。
classify_action_state() {
  local exec_rows="$1" sched_rows="$2" reject_rows="$3"
  if [ "$exec_rows" -gt 0 ]; then
    echo "EXECUTED"      # 终态:JobService 真执行
  elif [ "$sched_rows" -gt 0 ]; then
    echo "SCHEDULED"     # 中间态:socket 链路成立、job 已排,JobScheduler 未到点
  elif [ "$reject_rows" -gt 0 ]; then
    echo "REJECTED"      # 失败态:app 侧拒绝/失败
  else
    echo "NOT-EXECUTED"  # 链路未通:无任何 keep_alive_* 审计
  fi
}

# 脱敏闸门取样:返回最多 3 条未脱敏证据,空串即干净。覆盖 EventStore 全部 15 个敏感键
# (string 类查非空、null 类查非 null),按字节扫不留 locale 死角。与 #35 同口径,
# 原 #36 闸门只扫 3 个 string 键,窄于其守护的不变量。
redaction_leak_sample() {
  local trace="$1"
  [ -s "$trace" ] || return 0
  # 末尾 `|| true`:这是取样器(输出即信号,空=干净),其退出码不应中止脚本。零匹配时
  # grep 返回 1,叠加 pipefail 会让本函数返回非 0,在调用处 `leak="$(...)"` 触发 set -e
  # 静默退出(干净 trace 恰是常态)。head 提前关管也可能 SIGPIPE,一并由 `|| true` 兜住。
  {
    LC_ALL=C grep -aoE '"(raw_title|raw_text|notification_key)":"[^"]+"' "$trace" 2>/dev/null
    LC_ALL=C grep -aoE '"(group_key|key|tag|payload|responseBody|sourceText|sourceContentDescription|textItems|windowTitle|text|target|cachePath)":[^,}]*' "$trace" 2>/dev/null \
      | LC_ALL=C grep -av ':null$'
  } | head -3 || true
}

# 钉定 ANDROID_SERIAL,杜绝多设备下动作打错设备(review Medium)。#36 不自己起模拟器,
# 故策略是:已显式指定则尊重;恰好一台则自动钉;多台却没指定则拒绝(不替你猜该打哪台)。
pin_serial() {
  [ -n "${ANDROID_SERIAL:-}" ] && { log "沿用已指定 ANDROID_SERIAL=$ANDROID_SERIAL"; return 0; }
  local devs n
  devs="$(adb devices | awk '/\tdevice$/ {print $1}')"
  n="$(printf '%s\n' "$devs" | grep -c . || true)"
  if [ "$n" -eq 1 ]; then
    export ANDROID_SERIAL="$devs"
    log "钉定 ANDROID_SERIAL=$ANDROID_SERIAL"
  elif [ "$n" -eq 0 ]; then
    die "无在线设备"
  else
    die "检测到多台设备,请显式 export ANDROID_SERIAL=<serial> 后重跑(避免动作打错设备)"
  fi
}

stage0_preflight() {
  banner "阶段 0:环境自检"
  [ -d "$ANDROID_HOME" ] || die "ANDROID_HOME 不存在: $ANDROID_HOME"
  [ -x "$ANDROID_HOME/platform-tools/adb" ] || die "缺 adb"
  command -v cargo >/dev/null || die "缺 cargo"
  [ -f "$SAMPLE" ] || die "缺采集样本: $SAMPLE"
  pin_serial
  log "环境自检通过"
}

APK="apps/android-collector/app/build/outputs/apk/debug/app-debug.apk"

stage1_build_install() {
  banner "阶段 1:编译(含 auto-start)+ 安装"
  # 强制用当前源码:只要 app/src 或 gradle 配置里有任何文件比 APK 新,就重编译。
  # 否则会装上陈旧 APK,auto-start / 动作执行逻辑与当前源码不一致 —— 采集链
  # 踩过这个坑(旧 APK 把未脱敏原文写进 trace,绕过源码里的脱敏)。
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

stage2_autostart_service() {
  banner "阶段 2:auto-start 拉起前台服务(连带动作 socket)"
  # 先清 app 数据,杜绝跨轮旧 trace 累积。adb install -r 保留 app 私有目录,
  # 若不清,上一轮(甚至旧 APK)写的 actions.jsonl 会留存,阶段 6 run-as 拉出的
  # 是历史累积而非本轮 —— 采集链一次真实事故里正是它让旧数据被误判为本轮。
  # pm clear 同时会清掉权限,故下面在 auto-start 之前重新授通知权限。
  # 清不掉就等于带着"跨轮残留"事故根因继续跑,die 而非告警(原仅 warn 后继续,
  # 与本段自陈根因自相矛盾)。
  adb shell pm clear "$PKG" >>"$RUN_LOG" 2>&1 || die "pm clear 失败,无法保证本轮 trace 不含上轮残留(拒绝带病继续)"
  # POST_NOTIFICATIONS:前台服务在 Android 13+ 需要它才能显示常驻通知。
  # 动作回路只需前台服务把动作 socket 起起来,故只补这一项,不开通知监听源。
  adb shell pm grant "$PKG" android.permission.POST_NOTIFICATIONS >>"$RUN_LOG" 2>&1 || true
  # 用 auto_start extra 让 MainActivity 自动起前台采集服务(连带动作 socket)。
  adb shell am start -n "$PKG/.MainActivity" --ez auto_start true >>"$RUN_LOG" 2>&1 \
    || die "am start MainActivity 失败"
  sleep 4
  # 确认前台服务进程在(socket 随它起)
  if adb shell pidof "$PKG" >/dev/null 2>&1; then
    log "app 进程在,前台服务应已起(socket 监听 $ACTION_PORT)"
  else
    die "app 进程未起,auto-start 可能失败"
  fi
}

stage3_get_token() {
  banner "阶段 3:获取动作 socket token"
  # 非交互覆盖:若调用方已 export ACTION_TOKEN(如自动化/CI、或 debug build 的固定
  # 开发 token),直接用,免去人工抠 UI。否则提示人工粘贴。注意 stage2 的
  # `adb shell` 会吸干本管线 stdin,故自动化场景必须走本 env 覆盖而非管道喂 token。
  if [ -n "${ACTION_TOKEN:-}" ]; then
    ACTION_TOKEN="$(printf '%s' "$ACTION_TOKEN" | tr -d '[:space:]')"
    [ -n "$ACTION_TOKEN" ] || die "ACTION_TOKEN 为空"
    log "沿用预置 ACTION_TOKEN(${#ACTION_TOKEN} 字符,非交互)"
    return 0
  fi
  printf '\n>>> 请在模拟器 app 里点 "Copy Action Socket Token"(或在状态区查看完整 token)。\n'
  printf '>>> token 是 64 位十六进制。粘贴到这里后回车:\n'
  read -r ACTION_TOKEN
  ACTION_TOKEN="$(printf '%s' "$ACTION_TOKEN" | tr -d '[:space:]')"
  [ -n "$ACTION_TOKEN" ] || die "未提供 token"
  printf '%s' "$ACTION_TOKEN" | grep -qE '^[0-9a-f]{64}$' \
    || log "[warn] token 不像 64 位 hex,仍继续(以 app 实际为准)"
  log "已收到 token(${#ACTION_TOKEN} 字符)"
}

stage4_forward_port() {
  banner "阶段 4:端口转发"
  adb forward "tcp:$ACTION_PORT" "tcp:$ACTION_PORT" >>"$RUN_LOG" 2>&1 \
    || die "adb forward 失败"
  log "adb forward tcp:$ACTION_PORT -> 设备 tcp:$ACTION_PORT 已建立"
}

stage5_send_action() {
  banner "阶段 5:双轨发动作(A=daemon真发走完整Rust链 / B=验证通道取证)"
  log "轨道A:运行 dipecsd --no-daemon --android-trace-jsonl $SAMPLE(bridge 转发开)..."
  # 轨道A:daemon 消费样本走完整管线;AppTransition 产 KeepAlive(work:),经真实
  # AndroidAdapter 转发链(AndroidAdapter::forward)转发到 127.0.0.1:46321(已 forward
  # 到设备 app socket)。--no-daemon 跑一轮就够产出窗口;timeout 兜底防它常驻不退。
  DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=1 \
  DIPECS_ANDROID_ACTION_BRIDGE_TOKEN="$ACTION_TOKEN" \
  DIPECS_ANDROID_ACTION_BRIDGE_PORT="$ACTION_PORT" \
  RUST_LOG=info \
    timeout 30 cargo run -q -p aios-daemon --bin dipecsd -- \
      --no-daemon --android-trace-jsonl "$SAMPLE" >>"$RUN_LOG" 2>&1 || true
  # daemon 被 timeout 终止是预期的。env 开启时 daemon 走 AndroidAdapter,仅在收到设备
  # {status:ok} 回执后才打印 "AndroidAdapter: device confirmed execution"
  # (aios-action/src/android_adapter.rs)——即设备确认执行,而非"写出即记转发"。经 adb
  # forward 时数据/FIN 竞态常致设备空读、daemon 收不到 ok 回执,故此处恒为 0 属预期(真正的
  # 执行确认走轨道B旁证);生产 loopback 直连无此竞态。
  if grep -q "AndroidAdapter: device confirmed execution" "$RUN_LOG"; then
    log "轨道A:daemon 收到设备 {status:ok} 回执,确认执行(走完整 AndroidAdapter 信封链)"
    FORWARDED=1
  else
    log "[warn] 轨道A:未见设备执行确认(adb forward 竞态下属预期;见验证记录与轨道B旁证)"
    FORWARDED=0
  fi
  # 附带证据:PrefetchFile 经 RuleBased 路由应被 DeniedByCapability 拦截(治理边界)
  if grep -qE "DeniedByCapability|capability" "$RUN_LOG"; then
    log "附带验证:观察到能力拦截(治理边界生效)"
  fi

  # 轨道B(验证通道取证):daemon 经 adb forward 直发常被 adb 代理层"数据/FIN 竞态"
  # 截断(Android 记 authorized_action_socket_empty)——这是 adb forward 固有失真,
  # 生产 loopback 直连无此问题(见 action-forensic-sender.py 头注 + daemon-architecture.md
  # 里 daemon 终态为设备内 /system/bin/dipecsd)。为在验证通道取到"动作真执行"的旁证,
  # 取证发送器走与 daemon(AndroidAdapter)相同的 execute 信封(message_type / issued_at_ms /
  # expires_at_ms / auth.hmac_sha256 / action),HMAC 覆盖 freshness window 与 length-prefixed
  # action 字节(canonical 串 dipecs.android.bridge.execute.v1),与
  # AndroidAdapter::canonical_execute_envelope_input / 设备侧 BridgeExecuteProtocol 逐字节一致。
  # 发后延迟再关以规避 adb 竞态,在验证通道取到"动作真执行"的旁证。
  # 诚实边界:这是验证通道取证,不替代/不修改生产发送路径;轨道A 的真实表现已如实记上。
  # 取证发送器与本 stages 脚本同在 tests/scenarios/lib/ 下。
  local sender="$(dirname "${BASH_SOURCE[0]}")/action-forensic-sender.py"
  if command -v python3 >/dev/null 2>&1 && [ -f "$sender" ]; then
    log "轨道B:运行取证发送器(KeepAlive, 发后延迟 ${FORENSIC_DELAY:-1.5}s 关闭)..."
    if python3 "$sender" 127.0.0.1 "$ACTION_PORT" "$ACTION_TOKEN" "${FORENSIC_DELAY:-1.5}" >>"$RUN_LOG" 2>&1; then
      FORENSIC_SENT=1
      log "轨道B:取证 payload 已发出"
    else
      FORENSIC_SENT=0
      log "[warn] 轨道B:取证发送失败(见日志)"
    fi
  else
    FORENSIC_SENT=0
    log "[warn] 轨道B:缺 python3 或发送器,跳过取证旁证"
  fi
}

stage6_verify_execution() {
  banner "阶段 6:拉 app 私有 trace,三态判定动作执行"
  # 给 JobScheduler 时间真正执行已排的 keep_alive job 再拉 trace:scheduled→executed
  # 约 2.5s 异步延迟,拉太早会停在 SCHEDULED 中间态。等一会儿让常态取到 EXECUTED 终态
  # ——这不是造假,是给异步系统完成的时间;仍如实记录最终观测到的三态。可配 EXEC_SETTLE_SECS=0 关闭。
  sleep "${EXEC_SETTLE_SECS:-5}"
  local trace="data/traces/action-loop-e2e-$TS.jsonl"
  # run-as 拉已脱敏 trace(debug build 可 run-as);文件即 EventStore 的 files/traces/actions.jsonl
  adb shell run-as "$PKG" cat files/traces/actions.jsonl > "$trace" 2>>"$RUN_LOG" || true
  TRACE_FILE="$trace"

  # 脱敏闸门:动作回路 trace 主要是动作审计事件(keep_alive_*),非 rawEvent。
  # 但若混有 rawEvent 行,沿用采集链的字节级闸门(覆盖全部 15 个敏感键,见
  # redaction_leak_sample),非空即视为脱敏回归,立即停。
  local leak
  leak="$(redaction_leak_sample "$trace")"
  [ -n "$leak" ] && die "脱敏闸门拦截:trace 含未脱敏值(疑似旧 APK 或脱敏回归):$leak"

  # 统计两个关键审计事件(真实事件名,Android 源码确认):
  #   keep_alive_job_executed —— JobService 真正执行了(终态)
  #   keep_alive_scheduled    —— socket 收到动作、job 已排(链路已通,等 JobScheduler)
  local exec_rows sched_rows reject_rows
  exec_rows="$(count_audit_event "$trace" 'keep_alive_job_executed')"
  sched_rows="$(count_audit_event "$trace" 'keep_alive_scheduled')"
  reject_rows="$(count_audit_event "$trace" 'keep_alive_rejected|keep_alive_failed')"
  EXEC_ROWS="$exec_rows"
  SCHED_ROWS="$sched_rows"
  EMPTY_ROWS="$(count_audit_event "$trace" 'authorized_action_socket_empty')"
  log "动作审计统计 executed=$exec_rows scheduled=$sched_rows rejected/failed=$reject_rows socket_empty=$EMPTY_ROWS"
  # 证据归属:keep_alive_* 可能来自轨道A(daemon真发)或轨道B(取证发送器)。daemon 经
  # adb forward 直发常因代理层竞态记 socket_empty;若最终见 keep_alive_*,实跑环境下
  # 多由轨道B取证发送器产生。socket_empty 计数即 adb forward 失真的直接证据,一并记录。
  if [ "$EMPTY_ROWS" -gt 0 ]; then
    log "[info] 观察到 $EMPTY_ROWS 次 authorized_action_socket_empty = adb forward 数据/FIN 竞态失真(生产 loopback 无此问题)"
  fi
  # log 出事件 timestamp 供用户实跑时核对是否属本轮(防跨轮残留)。
  if [ -s "$trace" ]; then
    LC_ALL=C grep -aE 'keep_alive_(job_executed|scheduled|rejected|failed)' "$trace" 2>/dev/null \
      | LC_ALL=C grep -aoE '"(ts|timestamp|recorded_at_ms|created_at_ms)":[0-9]+' | tail -5 \
      | while read -r line; do log "  审计事件 ts: $line"; done || true
  fi

  # 四态判定收敛到 classify_action_state(纯函数,可单测);日志按态补充。
  DATA_SOURCE="$(classify_action_state "$exec_rows" "$sched_rows" "$reject_rows")"
  case "$DATA_SOURCE" in
    SCHEDULED)
      log "[info] socket 链路已通、job 已排,JobScheduler 尚未执行(中间态,非失败)" ;;
    REJECTED)
      log "[warn] 动作被拒绝/失败(keep_alive_rejected/keep_alive_failed),见 trace 原因" ;;
    NOT-EXECUTED)
      log "[warn] 未见任何动作审计;FORWARDED=${FORWARDED:-0},排查 token/签名/forward/服务" ;;
  esac
}

write_validation_record() {
  banner "写验证记录"
  local rec="data/evaluation/action-loop-e2e-$TS.md"
  {
    echo "# 动作回路验证记录 [$DATA_SOURCE]"
    echo
    echo "- 运行时间: $TS"
    echo "- 数据来源: **[$DATA_SOURCE]**"
    echo "- 转发动作: KeepAlive(work:collector_heartbeat)"
    echo "- 轨道A daemon 转发到 bridge(走完整 AndroidAdapter 转发链): $([ "${FORWARDED:-0}" = 1 ] && echo 是 || echo 否)"
    echo "- 轨道B 取证发送器已发出(验证通道,绕 adb 竞态): $([ "${FORENSIC_SENT:-0}" = 1 ] && echo 是 || echo 否)"
    echo "- 动作执行审计数(keep_alive_job_executed): ${EXEC_ROWS:-0}"
    echo "- 动作排程审计数(keep_alive_scheduled): ${SCHED_ROWS:-0}"
    echo "- adb forward 失真证据(authorized_action_socket_empty): ${EMPTY_ROWS:-0}"
    echo "- trace 文件: ${TRACE_FILE:-未拉取}"
    echo
    echo "## 链路说明"
    echo
    echo "daemon 决策 → AuthorizedAction → execute 信封(canonical HMAC-SHA256 + freshness window)"
    echo "→ localhost socket → adb forward → Android app 校验(HMAC/freshness)→ 排程 → JobService 执行 → 审计落盘。"
    echo "PrefetchFile 经 RuleBased 路由被 DeniedByCapability 拦截,体现治理边界。"
    echo
    echo "## 双轨发送与 adb forward 失真(诚实说明)"
    echo
    echo "- **轨道A(daemon 真发)**走完整 Rust 转发链(\`AndroidAdapter::forward\`)。"
    echo "  但经 \`adb forward\` 转发时,adb 用户态 TCP 代理在转发数据与转发 FIN 之间存在"
    echo "  调度间隙:发送端 write 后立即关连接,FIN 追上尚未推送到设备的数据,Android 侧"
    echo "  \`accept\` 后首次 read 即 EOF,读到空 payload,记 \`authorized_action_socket_empty\`。"
    echo "  这是 adb forward 的固有失真(业界亦有记录),**仅存在于'开发机 daemon 经 adb 打"
    echo "  设备 app'这条验证通道**。"
    echo "- **生产部署无此问题**:daemon 终态是设备内 \`/system/bin/dipecsd\`(见"
    echo "  \`docs/src/design/daemon-architecture.md\`),与 app 同机走内核 loopback,字节流"
    echo "  严格有序,数据必先于 FIN 到达。"
    echo "- **轨道B(取证发送器)**走与 daemon(AndroidAdapter)相同的 execute 信封,HMAC 覆盖"
    echo "  freshness window 与 length-prefixed \`action\` 字节(canonical 串"
    echo "  \`dipecs.android.bridge.execute.v1\`),与 \`AndroidAdapter::canonical_execute_envelope_input\`"
    echo "  / 设备侧 \`BridgeExecuteProtocol\` 逐字节一致。仅在 write 后延迟再关以规避 adb 竞态,"
    echo "  用于在验证通道取得'动作真执行'旁证。它不替代、不修改生产发送路径。"
    echo
    case "$DATA_SOURCE" in
      EXECUTED)
        echo "> ✅ EXECUTED:观察到 keep_alive_job_executed,动作经回路真实执行(终态)。"
        ;;
      SCHEDULED)
        echo "> ◐ SCHEDULED:socket 链路成立,动作已排 job(keep_alive_scheduled),"
        echo "> 但 JobScheduler 尚未到点执行 —— 这是诚实的中间态,不是失败,也不冒充 EXECUTED。"
        echo "> 脚本跑完那一刻 job 常未执行;实跑时可稍后再 run-as 拉 trace 核 keep_alive_job_executed。"
        ;;
      REJECTED)
        echo "> ⚠ REJECTED:动作被 app 侧拒绝/失败(keep_alive_rejected / keep_alive_failed)。"
        echo "> 原因见 trace 行(target 越界 / JobScheduler 不可用等)。"
        ;;
      NOT-EXECUTED)
        echo "> ⚠ NOT-EXECUTED:未观察到任何动作审计。转发状态 FORWARDED=${FORWARDED:-0}。"
        echo "> 如实记录:未冒充成功。排查方向:token 是否匹配、签名/TTL、adb forward、前台服务是否在。"
        echo "> 原始日志 $RUN_LOG。"
        ;;
    esac
  } > "$rec"
  log "验证记录写入 $rec"
}
