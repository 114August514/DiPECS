#!/usr/bin/env bash
# emulator-e2e 纯逻辑自测:不依赖 adb / 模拟器 / cargo,只对 stage 里抽出的判定函数
# (count_real_raw_events / classify_data_tag / redaction_leak_sample)喂 fixture 断言。
# 跑法:bash tests/scenarios/lib/emulator-e2e-selftest.sh  (退出码 0=全过)
#
# 这些用例正是 review 三个缺陷的回归锚点:
#   - 空采集(仅引导行)不得被数成 ≥1 → 不得误判 REAL
#   - manual 模式的 REAL 不得被无条件打成"部分源"
#   - 脱敏闸门要覆盖全部敏感键(string 类 + null 类),且不放过、也不误报
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# 被测 lib 顶层会用到这几个变量(NOTIF_SVC 引用 $PKG),source 前先给默认值,
# 否则 set -u 下 source 即报错。这些值只为让 source 通过,自测不碰 adb。
PKG="com.dipecs.collector"
RUN_LOG="/dev/null"
# shellcheck source=/dev/null
source "$SELF_DIR/emulator-e2e-stages.sh"

PASS=0
FAIL=0
ok()  { PASS=$((PASS + 1)); printf '  ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL %s\n         expected=[%s]\n         actual  =[%s]\n' "$1" "$2" "$3"; }
eq()  { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "$2" "$3"; fi; }
nonempty() { if [ -n "$2" ]; then ok "$1"; else bad "$1" "<非空>" "<空>"; fi; }
empty()    { if [ -z "$2" ]; then ok "$1"; else bad "$1" "<空>" "$2"; fi; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# fixture 1:空采集 —— 只有 listener 引导行(rawEvent:null)
printf '%s\n' \
  '{"eventType":"notification_listener_connected","rawEvent":null,"rawPayload":{}}' \
  > "$TMP/empty.jsonl"

# fixture 2:引导行 + 1 条真实通知(已脱敏:raw_title/raw_text="",group_key=null)
cat > "$TMP/one_notif.jsonl" <<'EOF'
{"eventType":"notification_listener_connected","rawEvent":null,"rawPayload":{}}
{"eventType":"notification_posted","rawEvent":{"NotificationPosted":{"raw_title":"","raw_text":"","group_key":null,"is_ongoing":false}},"rawPayload":{}}
EOF

# fixture 3:2 条 AppTransition、无通知(REAL 但部分源)
cat > "$TMP/apps_only.jsonl" <<'EOF'
{"eventType":"app_transition","rawEvent":{"AppTransition":{"to_package":"a"}}}
{"eventType":"app_transition","rawEvent":{"AppTransition":{"to_package":"b"}}}
EOF

# fixture 4:未脱敏 string 原文(raw_title 非空)
printf '%s\n' \
  '{"rawEvent":{"NotificationPosted":{"raw_title":"银行验证码 123456 …","raw_text":""}}}' \
  > "$TMP/leak_string.jsonl"

# fixture 5:未脱敏 null 类字段(target 应为 null 却带值)
printf '%s\n' \
  '{"rawEvent":{"PreWarm":{"target":"com.bank.app/.SecretActivity"}}}' \
  > "$TMP/leak_null.jsonl"

echo "== count_real_raw_events(只数 rawEvent 非空行)=="
eq  "空采集仅引导行 → 0"        "0" "$(count_real_raw_events "$TMP/empty.jsonl")"
eq  "引导行 + 1 真实 → 1"       "1" "$(count_real_raw_events "$TMP/one_notif.jsonl")"
eq  "2 条 AppTransition → 2"    "2" "$(count_real_raw_events "$TMP/apps_only.jsonl")"
eq  "缺失文件 → 0(单行不报错)" "0" "$(count_real_raw_events "$TMP/does_not_exist.jsonl")"

echo "== classify_data_tag(REAL 细分 / 与 auto-manual 无关)=="
eq  "FALLBACK 原样"                 "FALLBACK"      "$(classify_data_tag "FALLBACK" "$TMP/one_notif.jsonl")"
eq  "REAL + 有通知 → REAL"          "REAL"          "$(classify_data_tag "REAL" "$TMP/one_notif.jsonl")"
eq  "REAL + 无通知 → REAL(部分源)" "REAL(部分源)"  "$(classify_data_tag "REAL" "$TMP/apps_only.jsonl")"

echo "== redaction_leak_sample(覆盖全部敏感键,既不漏也不误报)=="
empty    "已脱敏 trace 判干净"        "$(redaction_leak_sample "$TMP/one_notif.jsonl")"
empty    "纯 AppTransition 判干净"    "$(redaction_leak_sample "$TMP/apps_only.jsonl")"
empty    "空采集判干净"              "$(redaction_leak_sample "$TMP/empty.jsonl")"
nonempty "string 类原文被拦截"        "$(redaction_leak_sample "$TMP/leak_string.jsonl")"
nonempty "null 类原文被拦截"          "$(redaction_leak_sample "$TMP/leak_null.jsonl")"

echo
printf '结果:PASS=%d FAIL=%d\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
