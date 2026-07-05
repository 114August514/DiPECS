#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
ADB="${ADB:-adb}"
PYTHON="${PYTHON:-python3}"
PACKAGE="${PACKAGE:-com.dipecs.collector}"
MODE="${MODE:-emulator-dry-run}"
SAMPLES="${SAMPLES:-20}"
CALIBRATION_SAMPLES="${CALIBRATION_SAMPLES:-5}"
SAMPLE_INTERVAL_SECS="${SAMPLE_INTERVAL_SECS:-1}"
PRESSURE_START_MB="${PRESSURE_START_MB:-128}"
PRESSURE_STEP_MB="${PRESSURE_STEP_MB:-128}"
PRESSURE_HOLD_MB="${PRESSURE_HOLD_MB:-$PRESSURE_START_MB}"
MAX_PRESSURE_MB="${MAX_PRESSURE_MB:-1536}"
PRESSURE_CHUNK_MB="${PRESSURE_CHUNK_MB:-16}"
PRESSURE_WINDOW_SECS="${PRESSURE_WINDOW_SECS:-10}"
PRESSURE_RAMP_SECS="${PRESSURE_RAMP_SECS:-2}"
PRESSURE_PROCESS_MB="${PRESSURE_PROCESS_MB:-256}"
MAX_PRESSURE_SLOTS="${MAX_PRESSURE_SLOTS:-8}"
TEMPERATURE_STOP_C="${TEMPERATURE_STOP_C:-42}"
MIN_AVAILABLE_STOP_MB="${MIN_AVAILABLE_STOP_MB:-256}"
MIN_AVAILABLE_STOP_MEMTOTAL_PCT="${MIN_AVAILABLE_STOP_MEMTOTAL_PCT:-5}"
TOKEN="${TOKEN:-dipecs-dev-emulator-shared-token-00000000}"
PORT="${PORT:-46321}"
ACTION_HOST="${ACTION_HOST:-127.0.0.1}"
DELAY="${DELAY:-1.0}"
OUT_DIR="${OUT_DIR:-$REPO_ROOT/data/evaluation/keepalive}"
LSAPP_REPORT="${LSAPP_REPORT:-$REPO_ROOT/data/evaluation/next-app/lsapp-standard.report.json}"
SENDER="$REPO_ROOT/tests/scenarios/lib/action-forensic-sender.py"
PRESSURE_SERVICE_COMPONENT="${PACKAGE}/.debug.DebugMemoryPressureService"

pressure_component() {
  local slot="$1"
  if [[ "$slot" -eq 0 ]]; then
    printf '%s/.debug.DebugMemoryPressureService\n' "$PACKAGE"
  else
    printf '%s/.debug.DebugMemoryPressureService%s\n' "$PACKAGE" "$slot"
  fi
}

pressure_process_name() {
  local slot="$1"
  if [[ "$slot" -eq 0 ]]; then
    printf '%s:pressure\n' "$PACKAGE"
  else
    printf '%s:pressure%s\n' "$PACKAGE" "$slot"
  fi
}

mkdir -p "$OUT_DIR"
timestamp="$(date +%Y%m%d-%H%M%S)"
raw_dir="$(mktemp -d)"
trap 'stop_pressure >/dev/null 2>&1 || true; rm -rf "$raw_dir"' EXIT

log() {
  printf '[keepalive-pressure] %s\n' "$*" >&2
}

die() {
  printf '[keepalive-pressure] ERROR: %s\n' "$*" >&2
  exit 1
}

adb_cmd() {
  "$ADB" "$@"
}

require_tools() {
  command -v "$ADB" >/dev/null 2>&1 || die "adb not found; set ADB=/path/to/adb"
  command -v "$PYTHON" >/dev/null 2>&1 || die "python3 not found; set PYTHON=/path/to/python3"
  [[ -f "$SENDER" ]] || die "action sender missing: $SENDER"
  [[ -f "$LSAPP_REPORT" ]] || die "LSApp report missing: $LSAPP_REPORT"
}

device_prop() {
  adb_cmd shell getprop "$1" 2>/dev/null | tr -d '\r' | head -n 1
}

meminfo_value_mb() {
  local key="$1"
  adb_cmd shell cat /proc/meminfo 2>/dev/null | tr -d '\r' |
    awk -v k="${key}:" '$1 == k { printf "%.3f", $2 / 1024; found=1 } END { if (!found) print "0" }'
}

memtotal_mb() {
  meminfo_value_mb MemTotal
}

available_mb() {
  meminfo_value_mb MemAvailable
}

target_pid() {
  adb_cmd shell pidof "$PACKAGE" 2>/dev/null | tr -d '\r' | awk '{print $1}' | head -n 1
}

pressure_pid() {
  local slot
  for ((slot=0; slot<MAX_PRESSURE_SLOTS; slot++)); do
    adb_cmd shell pidof "$(pressure_process_name "$slot")" 2>/dev/null | tr -d '\r' | awk '{print $1}' | head -n 1
  done | awk 'NF {print}' | head -n 1
}

pss_mb() {
  local process="$1" mem pss
  mem="$(adb_cmd shell dumpsys meminfo "$process" 2>/dev/null || true)"
  pss="$(printf '%s\n' "$mem" | sed -n 's/.*TOTAL PSS:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  "$PYTHON" - "$pss" <<'PY'
import sys
raw = sys.argv[1].strip()
print(round((int(raw) if raw else 0) / 1024.0, 3))
PY
}

gfxinfo() {
  local g total janky pct
  g="$(adb_cmd shell dumpsys gfxinfo "$PACKAGE" 2>/dev/null || true)"
  total="$(printf '%s\n' "$g" | sed -n 's/Total frames rendered:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  read -r janky pct < <(printf '%s\n' "$g" | sed -n 's/Janky frames:[[:space:]]*\([0-9][0-9]*\)[[:space:]]*(\([0-9.][0-9.]*\)%).*/\1 \2/p' | head -n 1)
  printf '%s %s %s\n' "${total:-0}" "${janky:-0}" "${pct:-0}"
}

battery_temp_c() {
  local raw temp
  raw="$(adb_cmd shell dumpsys battery 2>/dev/null || true)"
  temp="$(printf '%s\n' "$raw" | sed -n 's/[[:space:]]*temperature:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  "$PYTHON" - "$temp" <<'PY'
import sys
raw = sys.argv[1].strip()
print(round((int(raw) if raw else 0) / 10.0, 3))
PY
}

max_float() {
  "$PYTHON" - "$1" "$2" <<'PY'
import sys
print(max(float(sys.argv[1]), float(sys.argv[2])))
PY
}

safety_status() {
  local stage="$1" temp avail total min_allowed
  temp="$(battery_temp_c)"
  avail="$(available_mb)"
  total="$(memtotal_mb)"
  min_allowed="$(max_float "$MIN_AVAILABLE_STOP_MB" "$("$PYTHON" - "$total" "$MIN_AVAILABLE_STOP_MEMTOTAL_PCT" <<'PY'
import sys
print(float(sys.argv[1]) * float(sys.argv[2]) / 100.0)
PY
)")"
  "$PYTHON" - "$stage" "$temp" "$avail" "$total" "$min_allowed" "$TEMPERATURE_STOP_C" <<'PY'
import json
import sys

stage, temp, avail, total, min_allowed, temp_stop = sys.argv[1:]
temp = float(temp)
avail = float(avail)
total = float(total)
min_allowed = float(min_allowed)
temp_stop = float(temp_stop)
reason = None
if temp > 0 and temp >= temp_stop:
    reason = f"temperature_stop_c:{temp}"
elif avail > 0 and avail < min_allowed:
    reason = f"min_available_stop_mb:{avail}"
print(json.dumps({
    "stage": stage,
    "temperature_c": round(temp, 3),
    "available_mb": round(avail, 3),
    "memtotal_mb": round(total, 3),
    "min_available_allowed_mb": round(min_allowed, 3),
    "safe": reason is None,
    "reason": reason,
}))
PY
}

ensure_safety() {
  local status safe reason
  status="$(safety_status "$1")"
  safe="$(printf '%s' "$status" | "$PYTHON" -c 'import json,sys; print(json.load(sys.stdin)["safe"])')"
  if [[ "$safe" != "True" ]]; then
    reason="$(printf '%s' "$status" | "$PYTHON" -c 'import json,sys; print(json.load(sys.stdin)["reason"])')"
    printf '%s\n' "$status" > "$raw_dir/safety_stopped.json"
    log "safety_stopped: $reason"
    return 1
  fi
  return 0
}

start_collector() {
  adb_cmd forward "tcp:$PORT" "tcp:$PORT" >/dev/null
  adb_cmd shell am force-stop "$PACKAGE" >/dev/null 2>&1 || true
  sleep 1
  adb_cmd shell am start -n "$PACKAGE/.debug.DebugCollectorControlActivity" >/dev/null
  sleep 3
  local pid
  pid="$(target_pid)"
  [[ -n "$pid" ]] || die "target PID missing after starting collector"
}

home_screen() {
  adb_cmd shell input keyevent HOME >/dev/null 2>&1 || true
  sleep 1
}

return_to_target() {
  local result total
  result="$(adb_cmd shell am start -W -n "$PACKAGE/.MainActivity" 2>/dev/null || true)"
  total="$(printf '%s\n' "$result" | sed -n 's/.*TotalTime:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  if ! [[ "${total:-}" =~ ^[0-9]+$ ]] || (( total <= 0 )); then
    die "am start -W TotalTime missing or non-positive"
  fi
  printf '%s\n' "$total"
}

start_pressure() {
  local hold_mb="$1"
  local remaining="$hold_mb" slot slot_hold component
  for ((slot=0; slot<MAX_PRESSURE_SLOTS && remaining>0; slot++)); do
    slot_hold="$PRESSURE_PROCESS_MB"
    if (( remaining < slot_hold )); then
      slot_hold="$remaining"
    fi
    component="$(pressure_component "$slot")"
    adb_cmd shell am startservice \
      -n "$component" \
      -a com.dipecs.collector.debug.MEMORY_PRESSURE_START \
      --ei hold_mb "$slot_hold" \
      --ei chunk_mb "$PRESSURE_CHUNK_MB" \
      --ei window_secs "$PRESSURE_WINDOW_SECS" >/dev/null
    remaining=$((remaining - slot_hold))
  done
  if (( remaining > 0 )); then
    die "requested hold_mb=$hold_mb exceeds MAX_PRESSURE_SLOTS * PRESSURE_PROCESS_MB"
  fi
}

stop_pressure() {
  local slot component
  for ((slot=0; slot<MAX_PRESSURE_SLOTS; slot++)); do
    component="$(pressure_component "$slot")"
    adb_cmd shell am startservice \
      -n "$component" \
      -a com.dipecs.collector.debug.MEMORY_PRESSURE_STOP >/dev/null 2>&1 || true
  done
}

send_keepalive() {
  local issued_at_ms
  issued_at_ms="$(adb_cmd shell date +%s%3N 2>/dev/null | tr -d "\r" | head -n 1)"
  [[ "$issued_at_ms" =~ ^[0-9]+$ ]] || die "device epoch millis unavailable"
  "$PYTHON" - "$ACTION_HOST" "$PORT" "$TOKEN" "$DELAY" "$issued_at_ms" <<'PY'
import hashlib
import hmac
import json
import socket
import sys
import time

host, port, token, delay, issued_at_ms = sys.argv[1:]
port = int(port)
delay = float(delay)
issued_at_ms = int(issued_at_ms)
expires_at_ms = issued_at_ms + 60_000
action_json = json.dumps({
    "intent_id": "keepalive-memory-pressure",
    "coord": {"window_ordinal": 0, "intent_ordinal": 0, "action_ordinal": 0},
    "action": {
        "action_type": "KeepAlive",
        "target": "work:collector_heartbeat",
        "urgency": "Immediate",
    },
    "effect": "LocalStateChange",
    "authorized_at_ms": issued_at_ms,
})
canonical = (
    "dipecs.android.bridge.execute.v1\n"
    f"issued_at_ms:{issued_at_ms}\n"
    f"expires_at_ms:{expires_at_ms}\n"
    f"action:{len(action_json.encode('utf-8'))}:{action_json}"
)
tag = hmac.new(token.encode(), canonical.encode("utf-8"), hashlib.sha256).hexdigest()
payload = json.dumps({
    "message_type": "execute",
    "issued_at_ms": issued_at_ms,
    "expires_at_ms": expires_at_ms,
    "auth": {"hmac_sha256": tag},
    "action": action_json,
})
with socket.create_connection((host, port), timeout=8) as sock:
    sock.sendall(payload.encode("utf-8"))
    time.sleep(delay)
    sock.shutdown(socket.SHUT_WR)
    sock.settimeout(4)
    raw = sock.recv(1024)
if not raw:
    raise SystemExit("send_keepalive missing device response; expected status=ok or status=error response")
data = json.loads(raw.decode("utf-8", "replace"))
latency = int(data.get("latency_us") or 0)
if latency <= 0:
    raise SystemExit("send_keepalive missing positive latency_us")
print(json.dumps({
    "status": data.get("status"),
    "summary": data.get("summary"),
    "error": data.get("error"),
    "latency_us": latency,
    "system_action_ok": data.get("status") == "ok",
}, ensure_ascii=False))
PY
}

clear_logcat() {
  adb_cmd logcat -c >/dev/null 2>&1 || true
}

# Count SYSTEM-level memory-pressure kills only. Two exclusions matter:
#   1. The harness's OWN pressure processes (:pressure..:pressure7) are DESIGNED
#      to be LMKD-killed — counting them would treat self-inflicted churn as
#      evidence of native memory pressure (reviewer C2).
#   2. Lines matching our filter that also name a pressure slot as the victim are
#      dropped. We match kill signatures, then grep -v the pressure process name.
oom_event_count() {
  adb_cmd logcat -d -v brief 2>/dev/null |
    grep -Ei 'lowmemorykiller|lmkd|am_kill|Killing[[:space:]].*cached' |
    grep -Ev "${PACKAGE//./\\.}:pressure" |
    grep -c . || true
}

observe_pressure_window() {
  local min_available pressure_peak ppid i avail pss
  min_available="$(available_mb)"
  pressure_peak=0
  sleep "$PRESSURE_RAMP_SECS"
  for ((i=0; i<PRESSURE_WINDOW_SECS; i++)); do
    avail="$(available_mb)"
    min_available="$(min_float "$min_available" "$avail")"
    ppid="$(pressure_pid || true)"
    if [[ -n "$ppid" ]]; then
      pss="$(pressure_pss_mb)"
      pressure_peak="$(max_float "$pressure_peak" "$pss")"
    fi
    sleep 1
  done
  printf '%s %s\n' "$min_available" "$pressure_peak"
}

pressure_pss_mb() {
  local slot process total pss
  total=0
  for ((slot=0; slot<MAX_PRESSURE_SLOTS; slot++)); do
    process="$(pressure_process_name "$slot")"
    if adb_cmd shell pidof "$process" >/dev/null 2>&1; then
      pss="$(pss_mb "$process")"
      total="$(sum_float "$total" "$pss")"
    fi
  done
  printf '%s\n' "$total"
}

sum_float() {
  "$PYTHON" - "$1" "$2" <<'PY'
import sys
print(float(sys.argv[1]) + float(sys.argv[2]))
PY
}

min_float() {
  "$PYTHON" - "$1" "$2" <<'PY'
import sys
print(min(float(sys.argv[1]), float(sys.argv[2])))
PY
}

sample_once() {
  local mode="$1" idx="$2" hold_mb="$3" keepalive="$4"
  clear_logcat
  ensure_safety "before_sample" || return 2
  start_collector
  local pid_before available_before memtotal target_pss_before keepalive_json keepalive_status
  pid_before="$(target_pid)"
  available_before="$(available_mb)"
  memtotal="$(memtotal_mb)"
  target_pss_before="$(pss_mb "$PACKAGE")"
  keepalive_json='{}'
  keepalive_status="not_sent"
  if [[ "$keepalive" == "1" ]]; then
    keepalive_json="$(send_keepalive)"
    keepalive_status="$(printf '%s' "$keepalive_json" | "$PYTHON" -c 'import json,sys; print(json.load(sys.stdin).get("status") or "missing")')"
  fi
  home_screen
  ensure_safety "before_pressure" || return 2
  start_pressure "$hold_mb"
  local pressure_available_min pressure_pss_peak pid_after_pressure target_survived target_restarted return_total
  read -r pressure_available_min pressure_pss_peak < <(observe_pressure_window)
  pid_after_pressure="$(target_pid || true)"
  if [[ -n "$pid_after_pressure" && "$pid_after_pressure" == "$pid_before" ]]; then
    target_survived=true
  else
    target_survived=false
  fi
  stop_pressure
  ensure_safety "after_pressure" || return 2
  return_total="$(return_to_target)"
  local pid_after available_after target_pss_after total_frames janky_frames jank_pct oom_count
  pid_after="$(target_pid || true)"
  if [[ -z "$pid_after_pressure" || "$pid_after" != "$pid_before" ]]; then
    target_restarted=true
  else
    target_restarted=false
  fi
  available_after="$(available_mb)"
  target_pss_after="$(pss_mb "$PACKAGE")"
  read -r total_frames janky_frames jank_pct < <(gfxinfo)
  oom_count="$(oom_event_count)"
  "$PYTHON" - \
    "$idx" "$mode" "$pid_before" "$pid_after" "$target_survived" "$target_restarted" \
    "$return_total" "$keepalive_status" "$keepalive_json" "$pressure_available_min" \
    "$available_before" "$available_after" "$memtotal" "$target_pss_before" \
    "$target_pss_after" "$pressure_pss_peak" "$total_frames" "$janky_frames" \
    "$jank_pct" "$oom_count" "$hold_mb" <<'PY'
import json
import sys
import time

(
    idx,
    mode,
    pid_before,
    pid_after,
    survived,
    restarted,
    return_total,
    keepalive_status,
    keepalive_json,
    pressure_available_min,
    available_before,
    available_after,
    memtotal,
    target_pss_before,
    target_pss_after,
    pressure_pss_peak,
    total_frames,
    janky_frames,
    jank_pct,
    oom_count,
    hold_mb,
) = sys.argv[1:]
keepalive = json.loads(keepalive_json)
obj = {
    "sample_index": int(idx),
    "timestamp_ms": int(time.time() * 1000),
    "mode": mode,
    "target_pid_before": int(pid_before) if pid_before else None,
    "target_pid_after": int(pid_after) if pid_after else None,
    "target_survived": survived == "true",
    "target_restarted": restarted == "true",
    "return_total_time_ms": int(return_total),
    "keepalive_device_status": keepalive_status,
    "keepalive_summary": keepalive.get("summary"),
    "keepalive_error": keepalive.get("error"),
    "keepalive_latency_us": keepalive.get("latency_us", 0),
    "oom_score_adjusted": bool(keepalive.get("summary") and ":oom" in str(keepalive.get("summary"))),
    "cgroup_pinned": bool(keepalive.get("summary") and ":cgroup" in str(keepalive.get("summary"))),
    "pressure_hold_mb": int(hold_mb),
    "pressure_available_min_mb": round(float(pressure_available_min), 3),
    "available_before_mb": round(float(available_before), 3),
    "available_after_mb": round(float(available_after), 3),
    "memtotal_mb": round(float(memtotal), 3),
    "target_pss_before_mb": round(float(target_pss_before), 3),
    "target_pss_after_mb": round(float(target_pss_after), 3),
    "pressure_pss_peak_mb": round(float(pressure_pss_peak), 3),
    "total_frames": int(float(total_frames)),
    "janky_frames": int(float(janky_frames)),
    "jank_pct": round(float(jank_pct), 3),
    "oom_event_count": int(float(oom_count)),
}
print(json.dumps(obj, ensure_ascii=False))
PY
}

collect_mode() {
  local mode="$1" hold_mb="$2" keepalive="$3" samples="$4" out="$5"
  : > "$out"
  for ((i=0; i<samples; i++)); do
    local sample rc
    set +e
    sample="$(sample_once "$mode" "$i" "$hold_mb" "$keepalive")"
    rc=$?
    set -e
    if [[ "$rc" -eq 2 ]]; then
      assemble_report "safety_stopped" "$hold_mb"
      exit 3
    fi
    [[ "$rc" -eq 0 ]] || die "$mode sample $i failed"
    printf '%s\n' "$sample" >> "$out"
    "$PYTHON" - "$sample" <<'PY' >&2
import json
import sys
s = json.loads(sys.argv[1])
print(
    f"  {s['mode']}[{s['sample_index']}] survived={s['target_survived']} "
    f"restarted={s['target_restarted']} return={s['return_total_time_ms']}ms "
    f"avail_min={s['pressure_available_min_mb']}MB keepalive={s['keepalive_device_status']}"
)
PY
    sleep "$SAMPLE_INTERVAL_SECS"
  done
}

calibrate_pressure() {
  local background="$raw_dir/background_no_pressure.jsonl"
  local pilot="$raw_dir/pilot_no_keepalive_pressure.jsonl"
  log "calibration: background_no_pressure"
  collect_mode "background_no_pressure" 1 0 "$CALIBRATION_SAMPLES" "$background"
  local hold="$PRESSURE_START_MB"
  while (( hold <= MAX_PRESSURE_MB )); do
    log "calibration: pilot hold=${hold}MB"
    collect_mode "pilot_no_keepalive_pressure" "$hold" 0 "$CALIBRATION_SAMPLES" "$pilot"
    local valid
    valid="$("$PYTHON" - "$background" "$pilot" <<'PY'
import json
import math
import statistics
import sys

background = [json.loads(line) for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
pilot = [json.loads(line) for line in open(sys.argv[2], encoding="utf-8") if line.strip()]
def p95(vals):
    vals = sorted(vals)
    return vals[max(0, min(len(vals) - 1, math.ceil(0.95 * len(vals)) - 1))]
bg_p95 = p95([s["return_total_time_ms"] for s in background])
pilot_p95 = p95([s["return_total_time_ms"] for s in pilot])
death_or_restart = any((not s["target_survived"]) or s["target_restarted"] for s in pilot)
min_avail = min(s["pressure_available_min_mb"] for s in pilot)
memtotal = max(s["memtotal_mb"] for s in pilot)
oom = sum(s["oom_event_count"] for s in pilot)
latency_stress = pilot_p95 - bg_p95 >= 100 and min_avail < memtotal * 0.15
oom_stress = oom > 0 and min_avail < memtotal * 0.25
valid = death_or_restart or latency_stress or oom_stress
reason = "target_restart_observed" if death_or_restart else (
    "return_latency_and_low_available" if latency_stress else (
        "oom_signal_and_memory_competition" if oom_stress else "pressure_insufficient"
    )
)
print(json.dumps({"pressure_valid": valid, "reason": reason}))
PY
)"
    printf '%s\n' "$valid" > "$raw_dir/calibration.json"
    if [[ "$("$PYTHON" - "$valid" <<'PY'
import json, sys
print(json.loads(sys.argv[1])["pressure_valid"])
PY
)" == "True" ]]; then
      printf '%s\n' "$hold"
      return 0
    fi
    hold=$((hold + PRESSURE_STEP_MB))
  done
  printf '%s\n' "$MAX_PRESSURE_MB"
  return 1
}

assemble_report() {
  local status="$1" hold_mb="$2"
  local json_path="$OUT_DIR/keepalive-memory-pressure-real-device-$timestamp.json"
  local md_path="$OUT_DIR/keepalive-memory-pressure-real-device-$timestamp.md"
  local serial model release sdk
  serial="$(adb_cmd get-serialno 2>/dev/null | tr -d '\r' || echo unknown)"
  model="$(device_prop ro.product.model)"
  release="$(device_prop ro.build.version.release)"
  sdk="$(device_prop ro.build.version.sdk)"
  "$PYTHON" - \
    "$raw_dir" "$LSAPP_REPORT" "$json_path" "$md_path" "$status" "$MODE" "$serial" \
    "$model" "$release" "$sdk" "$PACKAGE" "$SAMPLES" "$CALIBRATION_SAMPLES" \
    "$hold_mb" "$MAX_PRESSURE_MB" "$TEMPERATURE_STOP_C" "$MIN_AVAILABLE_STOP_MB" \
    "$MIN_AVAILABLE_STOP_MEMTOTAL_PCT" <<'PY'
import datetime as dt
import json
import math
import pathlib
import statistics
import sys

(
    raw_dir,
    lsapp_report,
    json_path,
    md_path,
    status,
    mode,
    serial,
    model,
    release,
    sdk,
    package,
    samples,
    calibration_samples,
    hold_mb,
    max_pressure_mb,
    temperature_stop_c,
    min_available_stop_mb,
    min_available_stop_memtotal_pct,
) = sys.argv[1:]
raw = pathlib.Path(raw_dir)

def load(name):
    path = raw / name
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]

def summarize(samples):
    if not samples:
        return {"n": 0}
    def vals(field):
        return [float(s[field]) for s in samples]
    def p95(field):
        xs = sorted(vals(field))
        return round(xs[max(0, min(len(xs) - 1, math.ceil(0.95 * len(xs)) - 1))], 3)
    return {
        "n": len(samples),
        "survival_rate_pct": round(sum(1 for s in samples if s["target_survived"]) * 100 / len(samples), 3),
        "restart_rate_pct": round(sum(1 for s in samples if s["target_restarted"]) * 100 / len(samples), 3),
        "mean_return_total_time_ms": round(statistics.mean(vals("return_total_time_ms")), 3),
        "p95_return_total_time_ms": p95("return_total_time_ms"),
        "mean_jank_pct": round(statistics.mean(vals("jank_pct")), 3),
        "p95_jank_pct": p95("jank_pct"),
        "mean_available_after_mb": round(statistics.mean(vals("available_after_mb")), 3),
        "p95_pressure_available_min_mb": p95("pressure_available_min_mb"),
        "mean_target_pss_after_mb": round(statistics.mean(vals("target_pss_after_mb")), 3),
        "oom_event_count": int(sum(s["oom_event_count"] for s in samples)),
        "system_keepalive_ok_count": int(sum(1 for s in samples if s.get("keepalive_device_status") == "ok")),
        # oom_score_adj is the LOAD-BEARING KeepAlive mechanism (it decides LMKD
        # kill order). The cpuset pin is secondary and kernel-dependent
        # (pinToForegroundCgroup returns false when /dev/cpuset/foreground/tasks
        # is absent), so a legitimate system dipecsd that lowers oom but cannot
        # pin cgroup must still count as engaged. Track oom-primary engagement as
        # the gate, and full oom+cgroup separately for forensics.
        "oom_engaged_count": int(sum(1 for s in samples if s.get("oom_score_adjusted"))),
        "mechanism_engaged_count": int(sum(
            1 for s in samples if s.get("oom_score_adjusted") and s.get("cgroup_pinned")
        )),
    }

background = load("background_no_pressure.jsonl")
pilot = load("pilot_no_keepalive_pressure.jsonl")
no_keepalive = load("no_keepalive_pressure.jsonl")
keepalive = load("keepalive_pressure.jsonl")
calibration = {"pressure_valid": False, "native_baseline_stress_reason": "not_run"}
if (raw / "calibration.json").exists():
    c = json.loads((raw / "calibration.json").read_text(encoding="utf-8"))
    calibration = {
        "pressure_valid": bool(c.get("pressure_valid")),
        "native_baseline_stress_reason": c.get("reason", "unknown"),
    }

runs = []
for name, samples_list in [
    ("background_no_pressure", background),
    ("pilot_no_keepalive_pressure", pilot),
    ("no_keepalive_pressure", no_keepalive),
    ("keepalive_pressure", keepalive),
]:
    if samples_list:
        runs.append({"mode": name, "samples": samples_list, "summary": summarize(samples_list)})

summary = {run["mode"]: run["summary"] for run in runs}
pressure_valid = bool(calibration["pressure_valid"])
formal_n_ok = (
    len(no_keepalive) >= int(samples)
    and len(keepalive) >= int(samples)
    and int(samples) >= 20
)
if formal_n_ok:
    no_sum = summary["no_keepalive_pressure"]
    keep_sum = summary["keepalive_pressure"]
    survival_delta_pp = keep_sum["survival_rate_pct"] - no_sum["survival_rate_pct"]
    restart_delta_pp = no_sum["restart_rate_pct"] - keep_sum["restart_rate_pct"]
    return_p95_delta_ms = keep_sum["p95_return_total_time_ms"] - no_sum["p95_return_total_time_ms"]
    jank_delta_pp = keep_sum["mean_jank_pct"] - no_sum["mean_jank_pct"]
    available_after_delta_mb = keep_sum["mean_available_after_mb"] - no_sum["mean_available_after_mb"]
    oom_delta = keep_sum["oom_event_count"] - no_sum["oom_event_count"]
else:
    survival_delta_pp = restart_delta_pp = return_p95_delta_ms = jank_delta_pp = 0.0
    available_after_delta_mb = 0.0
    oom_delta = 0

target_benefit = survival_delta_pp + restart_delta_pp
system_cost = 0.0
if return_p95_delta_ms > 50:
    system_cost += return_p95_delta_ms / 50.0
if jank_delta_pp > 2:
    system_cost += jank_delta_pp
if available_after_delta_mb < -64:
    system_cost += abs(available_after_delta_mb) / 64.0
if oom_delta > 0:
    system_cost += oom_delta
net_score = target_benefit - system_cost

lsapp = json.load(open(lsapp_report, encoding="utf-8"))
examples = int(lsapp["test_examples"])
ensemble_hit = float(lsapp["metrics"]["ensemble"]["hit_rate_at_1_pct"])
strong_hit = float(lsapp["metrics"]["strong_predictive"]["hit_rate_at_1_pct"])

def scaled(hit):
    budget = examples
    hits = budget * hit / 100.0
    gross = hits * target_benefit
    cost = budget * system_cost
    return {
        "hit_rate_at_1_pct": round(hit, 3),
        "action_budget": budget,
        "gross_target_benefit": round(gross, 3),
        "system_cost": round(cost, 3),
        "net_benefit_score": round(gross - cost, 3),
    }

strong_baseline_comparison = {
    "dipecs_ensemble": scaled(ensemble_hit),
    "strong_predictive": scaled(strong_hit),
}

# native_baseline_hit: the no-keepalive arm must show a MEANINGFUL number of
# deaths/restarts, not a single flaky sample. At n>=20 require >=3 absolute
# kill-or-restart events (not just <95% survival, which 1/20 would satisfy) so a
# lone coincidental kill cannot green-light the gate (reviewer I1).
native_baseline_hit = False
if formal_n_ok:
    no_samples = summary["no_keepalive_pressure"]["n"]
    no_kill_or_restart = sum(
        1 for s in no_keepalive if (not s["target_survived"]) or s["target_restarted"]
    )
    native_baseline_hit = no_kill_or_restart >= 3

# MECHANISM ENGAGEMENT (reviewer C1): a KeepAlive *benefit* can only be
# attributed to KeepAlive if the KeepAlive action actually engaged its LOAD-
# BEARING system mechanism (oom_score_adj lowered) on EVERY keepalive-arm sample.
# We gate on oom-primary engagement, not oom+cgroup: the cpuset pin is secondary
# and kernel-dependent, so a legitimate platform-signed dipecsd that lowers oom
# but cannot pin cgroup must still qualify. On a non-privileged app oom is always
# denied (-> JobScheduler fallback), so acceptance is correctly impossible without
# a system deployment. Without this, a pure-noise survival delta would be
# miscredited to an inert KeepAlive.
keep_n = summary.get("keepalive_pressure", {}).get("n", 0)
mechanism_engaged = (
    formal_n_ok
    and keep_n > 0
    and summary["keepalive_pressure"]["oom_engaged_count"] == keep_n
)

user_cost = return_p95_delta_ms > 50 or jank_delta_pp > 2
memory_cost = available_after_delta_mb < -64 or oom_delta > 0
# directional_benefit: require a MINIMUM ABSOLUTE separation in events, not a
# one-sample 5pp artifact. survival/restart deltas must reflect >=3 more
# survivals (or >=3 fewer restarts) in the keepalive arm (reviewer I1).
if formal_n_ok:
    keep_survivors = sum(1 for s in keepalive if s["target_survived"])
    no_survivors = sum(1 for s in no_keepalive if s["target_survived"])
    keep_restarts = sum(1 for s in keepalive if s["target_restarted"])
    no_restarts = sum(1 for s in no_keepalive if s["target_restarted"])
    directional_benefit = (
        (survival_delta_pp >= 5 and (keep_survivors - no_survivors) >= 3)
        or (restart_delta_pp >= 5 and (no_restarts - keep_restarts) >= 3)
    )
else:
    directional_benefit = False

accepted = (
    status == "measured_android_real_device"
    and pressure_valid
    and formal_n_ok
    and mechanism_engaged
    and native_baseline_hit
    and directional_benefit
    and not user_cost
    and not memory_cost
    and net_score > 0
)

if status == "safety_stopped":
    conclusion_status = "safety_stopped"
    reason = "safety gate triggered"
elif not pressure_valid and status != "emulator_dry_run":
    conclusion_status = "pressure_insufficient"
    reason = "native Android baseline was not stressed enough"
elif formal_n_ok and not native_baseline_hit:
    conclusion_status = "pressure_insufficient"
    reason = "formal no-keepalive pressure did not kill or restart the native Android baseline"
elif accepted:
    conclusion_status = "accepted"
    reason = "KeepAlive improved target survival/restart without measured system cost"
else:
    conclusion_status = "not_significant"
    reason = "KeepAlive did not pass measured pressure benefit gates"

data = {
    "schema_version": "dipecs.keepalive_memory_pressure.v1",
    "issue": 98,
    "source": "measured_device" if status == "measured_android_real_device" else "emulator_or_partial",
    "status": status,
    "collected_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
    "device": {
        "serial": serial,
        "model": model,
        "android_release": release,
        "sdk": sdk,
    },
    "provenance": {
        "script": "tools/collect/collect-keepalive-memory-pressure.sh",
        "lsapp_report": "data/evaluation/next-app/lsapp-standard.report.json",
        "target_package": package,
        "samples_per_mode": int(samples),
        "calibration_samples": int(calibration_samples),
        "pressure_target": "derived_from_device_memavailable",
        "pressure_hold_mb": int(hold_mb),
        "max_pressure_mb": int(max_pressure_mb),
    },
    "safety": {
        "emulator_dry_run_required": True,
        "temperature_stop_c": float(temperature_stop_c),
        "min_available_stop_mb": float(min_available_stop_mb),
        "min_available_stop_memtotal_pct": float(min_available_stop_memtotal_pct),
        "safety_stopped": status == "safety_stopped",
    },
    "calibration": {
        "pressure_valid": pressure_valid,
        "background_no_pressure_samples": len(background),
        "pilot_samples": len(pilot),
        "native_baseline_stress_reason": calibration["native_baseline_stress_reason"],
    },
    "runs": runs,
    "summary": {
        "pressure_valid": pressure_valid,
        "formal_n_at_least_20_per_mode": formal_n_ok,
        "native_baseline_hit": native_baseline_hit,
        "survival_delta_pp": round(survival_delta_pp, 3),
        "restart_delta_pp": round(restart_delta_pp, 3),
        "return_p95_delta_ms": round(return_p95_delta_ms, 3),
        "jank_delta_pp": round(jank_delta_pp, 3),
        "available_after_delta_mb": round(available_after_delta_mb, 3),
        "oom_event_delta": oom_delta,
        "target_benefit": round(target_benefit, 3),
        "system_cost": round(system_cost, 3),
        "net_benefit_score": round(net_score, 3),
    },
    "strong_baseline_comparison": strong_baseline_comparison,
    "conclusion": {
        "pressure_valid": pressure_valid,
        "n_at_least_20_per_mode": formal_n_ok,
        "native_baseline_hit": native_baseline_hit,
        "directional_benefit": directional_benefit,
        "user_cost_absent": not user_cost,
        "memory_cost_absent": not memory_cost,
        "accepted": accepted,
        "status": conclusion_status,
        "reason": reason,
    },
}

pathlib.Path(json_path).write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
md = f"""# DiPECS KeepAlive Memory Pressure Measurement

- Status: {status}
- Conclusion: {conclusion_status}
- Accepted: {accepted}
- Device: {model} / Android {release} (serial {serial})
- Pressure hold: {hold_mb} MB
- Pressure valid: {pressure_valid} ({calibration['native_baseline_stress_reason']})

## Summary

| Metric | Value |
| --- | ---: |
| formal n>=20 per mode | {formal_n_ok} |
| native baseline hit | {native_baseline_hit} |
| survival delta | {survival_delta_pp:.3f} pp |
| restart delta | {restart_delta_pp:.3f} pp |
| return p95 delta | {return_p95_delta_ms:.3f} ms |
| jank delta | {jank_delta_pp:.3f} pp |
| available-after delta | {available_after_delta_mb:.3f} MB |
| net benefit score | {net_score:.3f} |

## Strong Baseline

| Model | hit@1 | action budget | net benefit score |
| --- | ---: | ---: | ---: |
| DiPECS ensemble | {strong_baseline_comparison['dipecs_ensemble']['hit_rate_at_1_pct']}% | {examples} | {strong_baseline_comparison['dipecs_ensemble']['net_benefit_score']} |
| StrongPredictiveActionBaseline | {strong_baseline_comparison['strong_predictive']['hit_rate_at_1_pct']}% | {examples} | {strong_baseline_comparison['strong_predictive']['net_benefit_score']} |

## Interpretation

{reason}
"""
pathlib.Path(md_path).write_text(md, encoding="utf-8")
print(json_path)
print(md_path)
PY
}

run_emulator_dry_run() {
  log "mode=emulator-dry-run samples=$SAMPLES"
  collect_mode "no_keepalive_pressure" "$PRESSURE_HOLD_MB" 0 "$SAMPLES" "$raw_dir/no_keepalive_pressure.jsonl"
  collect_mode "keepalive_pressure" "$PRESSURE_HOLD_MB" 1 "$SAMPLES" "$raw_dir/keepalive_pressure.jsonl"
  printf '{"pressure_valid": false, "reason": "emulator_dry_run"}\n' > "$raw_dir/calibration.json"
  assemble_report "emulator_dry_run" "$PRESSURE_HOLD_MB"
}

run_real_device_calibrate() {
  log "mode=real-device-calibrate"
  local hold rc
  set +e
  hold="$(calibrate_pressure)"
  rc=$?
  set -e
  if [[ "$rc" -eq 0 ]]; then
    assemble_report "pressure_valid" "$hold"
    return 0
  fi
  assemble_report "pressure_insufficient" "$hold"
  return 2
}

run_real_device_collect() {
  log "mode=real-device-collect samples=$SAMPLES"
  local hold
  hold="$(calibrate_pressure)" || {
    assemble_report "pressure_insufficient" "$MAX_PRESSURE_MB"
    return 2
  }
  collect_mode "no_keepalive_pressure" "$hold" 0 "$SAMPLES" "$raw_dir/no_keepalive_pressure.jsonl"
  collect_mode "keepalive_pressure" "$hold" 1 "$SAMPLES" "$raw_dir/keepalive_pressure.jsonl"
  assemble_report "measured_android_real_device" "$hold"
}

main() {
  require_tools
  case "$MODE" in
    emulator-dry-run) run_emulator_dry_run ;;
    real-device-calibrate) run_real_device_calibrate ;;
    real-device-collect) run_real_device_collect ;;
    *) die "unknown MODE=$MODE; expected emulator-dry-run, real-device-calibrate, or real-device-collect" ;;
  esac
}

main "$@"
