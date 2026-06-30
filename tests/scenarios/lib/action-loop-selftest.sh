#!/usr/bin/env bash
# action-loop-e2e 纯逻辑自测:不依赖 adb / 模拟器 / cargo,只对 stage 里抽出的判定函数
# (count_audit_event / classify_action_state / redaction_leak_sample)喂 fixture 断言。
# 跑法:bash tests/scenarios/lib/action-loop-selftest.sh  (退出码 0=全过)
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# 被测 lib 顶层会用到这几个变量,source 前给默认值(set -u 下否则 source 即报错)。
PKG="com.dipecs.collector"
RUN_LOG="/dev/null"
ACTION_PORT=46321
# shellcheck source=/dev/null
source "$SELF_DIR/action-loop-stages.sh"

PASS=0
FAIL=0
ok()  { PASS=$((PASS + 1)); printf '  ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL %s\n         expected=[%s]\n         actual  =[%s]\n' "$1" "$2" "$3"; }
eq()  { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "$2" "$3"; fi; }
nonempty() { if [ -n "$2" ]; then ok "$1"; else bad "$1" "<非空>" "<空>"; fi; }
empty()    { if [ -z "$2" ]; then ok "$1"; else bad "$1" "<空>" "$2"; fi; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# fixture:执行态(含 executed + scheduled,执行应优先)
cat > "$TMP/executed.jsonl" <<'EOF'
{"eventType":"keep_alive_scheduled","reason":"socket_authorized_action","ts":1}
{"eventType":"keep_alive_job_executed","reason":"socket_authorized_action","ts":2}
EOF

# fixture:仅排程(中间态)
printf '%s\n' '{"eventType":"keep_alive_scheduled","ts":1}' > "$TMP/scheduled.jsonl"

# fixture:被拒
printf '%s\n' '{"eventType":"keep_alive_rejected","reason":"target_out_of_scope"}' > "$TMP/rejected.jsonl"

# fixture:adb 竞态空 payload,无任何 keep_alive_*(链路未通)
printf '%s\n' '{"eventType":"authorized_action_socket_empty","ts":1}' > "$TMP/empty_socket.jsonl"

# fixture:已脱敏动作 trace(干净)
printf '%s\n' '{"eventType":"keep_alive_job_executed","target":null,"key":null}' > "$TMP/clean.jsonl"

# fixture:混入未脱敏 rawEvent string 原文
printf '%s\n' '{"rawEvent":{"NotificationPosted":{"raw_title":"验证码 99887 …","raw_text":""}}}' > "$TMP/leak.jsonl"

echo "== count_audit_event(零匹配返回干净单行 0)=="
eq "executed 计数 = 1"        "1" "$(count_audit_event "$TMP/executed.jsonl" 'keep_alive_job_executed')"
eq "scheduled 计数 = 1"       "1" "$(count_audit_event "$TMP/executed.jsonl" 'keep_alive_scheduled')"
eq "缺失事件 → 单行 0"        "0" "$(count_audit_event "$TMP/executed.jsonl" 'keep_alive_failed')"
eq "缺失文件 → 单行 0"        "0" "$(count_audit_event "$TMP/nope.jsonl" 'keep_alive_job_executed')"
# 零匹配必须是单行(防 [ -gt ] 报 integer expression expected)
eq "零匹配输出仅 1 行"        "1" "$(count_audit_event "$TMP/executed.jsonl" 'NOPE' | wc -l | tr -d ' ')"

echo "== classify_action_state(执行优先 → 中间 → 失败 → 未通)=="
eq "executed+scheduled → EXECUTED" "EXECUTED"     "$(classify_action_state 1 1 0)"
eq "仅 scheduled → SCHEDULED"      "SCHEDULED"    "$(classify_action_state 0 1 0)"
eq "仅 rejected → REJECTED"        "REJECTED"     "$(classify_action_state 0 0 1)"
eq "全 0 → NOT-EXECUTED"           "NOT-EXECUTED" "$(classify_action_state 0 0 0)"

echo "== redaction_leak_sample(动作 trace 也要守脱敏)=="
empty    "已脱敏动作 trace 判干净"   "$(redaction_leak_sample "$TMP/clean.jsonl")"
empty    "纯审计事件判干净"          "$(redaction_leak_sample "$TMP/scheduled.jsonl")"
nonempty "混入未脱敏原文被拦截"      "$(redaction_leak_sample "$TMP/leak.jsonl")"

echo
printf '结果:PASS=%d FAIL=%d\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
