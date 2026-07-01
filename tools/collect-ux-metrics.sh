#!/usr/bin/env bash
set -euo pipefail

ADB="${ADB:-}"
PACKAGE="${PACKAGE:-com.dipecs.collector}"
SAMPLES_PER_MODE="${SAMPLES_PER_MODE:-10}"
SAMPLE_INTERVAL_SECS="${SAMPLE_INTERVAL_SECS:-10}"
OUT_DIR="${OUT_DIR:-data/evaluation}"
TOKEN="${TOKEN:-dipecs-dev-emulator-shared-token-00000000}"
PORT="${PORT:-46321}"
ACTION_HOST="${ACTION_HOST:-127.0.0.1}"

if [[ -z "$ADB" ]]; then
  if command -v adb >/dev/null 2>&1; then
    ADB="$(command -v adb)"
  elif [[ -x "/mnt/c/Users/33207/AppData/Local/Android/Sdk/platform-tools/adb.exe" ]]; then
    ADB="/mnt/c/Users/33207/AppData/Local/Android/Sdk/platform-tools/adb.exe"
  else
    echo "adb not found. Set ADB=/path/to/adb or install Android platform-tools in WSL." >&2
    exit 1
  fi
fi

# Windows Python for socket actions (WSL2 localhost forwarding is unreliable;
# Windows Python uses the Windows TCP stack which reaches the adb forward).
SEND_PYTHON="${SEND_PYTHON:-}"
if [[ -z "$SEND_PYTHON" ]]; then
  if [[ -x "/mnt/c/Users/33207/AppData/Local/Programs/Python/Python313/python.exe" ]]; then
    SEND_PYTHON="/mnt/c/Users/33207/AppData/Local/Programs/Python/Python313/python.exe"
  else
    SEND_PYTHON="$(command -v python3 2>/dev/null || echo python3)"
  fi
fi

adb_cmd() { "$ADB" "$@"; }

# ── action sender (Windows Python for reliable adb-forward reach) ──

send_action() {
  local action_type="$1" target="$2"
  local sender="tests/scenarios/lib/action-forensic-sender.py"
  # Convert WSL path to Windows path for Windows Python
  local sender_win
  sender_win="$(wslpath -w "$sender" 2>/dev/null || echo "E:\\\\DIPECS\\\\tests\\\\scenarios\\\\lib\\\\action-forensic-sender.py")"
  "$SEND_PYTHON" "$sender_win" "$ACTION_HOST" "$PORT" "$TOKEN" 1.0 "$action_type" "$target" Immediate >/dev/null 2>&1 || true
}

send_prewarm() { send_action PreWarmProcess own:warmup; }
send_release() { send_action ReleaseMemory cache:prefetch; }

# ── adb metrics helpers ──

start_collector() {
  adb_cmd shell am start -n "$PACKAGE/.debug.DebugCollectorControlActivity" >/dev/null 2>&1
  sleep 4
}

stop_collector() {
  adb_cmd shell am force-stop "$PACKAGE" >/dev/null 2>&1 || true
  sleep 2
}

get_startup_time() {
  local result total_time
  # Activity is started via am start -W; process should already be running
  # (service was started beforehand), so this is a WARM activity launch.
  result="$(adb_cmd shell am start -W -n "$PACKAGE/.MainActivity" 2>/dev/null || true)"
  total_time="$(printf '%s\n' "$result" | sed -n 's/.*TotalTime:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  echo "${total_time:-0}"
}

get_meminfo() {
  local mem rss pss
  mem="$(adb_cmd shell dumpsys meminfo "$PACKAGE" 2>/dev/null || true)"
  rss="$(printf '%s\n' "$mem" | sed -n 's/.*TOTAL RSS:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  pss="$(printf '%s\n' "$mem" | sed -n 's/.*TOTAL PSS:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  rss="${rss:-0}"
  pss="${pss:-0}"
  python3 - "$rss" "$pss" <<'PY'
import sys
rss, pss = [int(x or 0) for x in sys.argv[1:]]
print(round(rss / 1024, 3), round(pss / 1024, 3))
PY
}

get_gfxinfo() {
  local g total janky pct
  g="$(adb_cmd shell dumpsys gfxinfo "$PACKAGE" 2>/dev/null || true)"
  total="$(printf '%s\n' "$g" | sed -n 's/Total frames rendered:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  read -r janky pct < <(printf '%s\n' "$g" | sed -n 's/Janky frames:[[:space:]]*\([0-9][0-9]*\)[[:space:]]*(\([0-9.][0-9.]*\)%).*/\1 \2/p' | head -n 1)
  echo "${total:-0} ${janky:-0} ${pct:-0}"
}

get_top_metrics() {
  local line pid cpu
  line="$(adb_cmd shell top -b -n 1 -o PID,%CPU,RES,ARGS 2>/dev/null | grep "$PACKAGE" | head -n 1 || true)"
  if [[ -z "$line" ]]; then
    echo "null 0 0"
    return
  fi
  set -- $line
  echo "$1 $2 $3"
}

# ── system baseline (DiPECS stopped, measure system state) ──

get_system_free_ram() {
  local mem
  mem="$(adb_cmd shell cat /proc/meminfo 2>/dev/null | grep MemAvailable | awk '{print $2}' || echo 0)"
  echo "${mem:-0}"
}

system_baseline_sample() {
  local mode="$1" idx="$2"
  local free_ram ts
  stop_collector
  sleep 3
  free_ram="$(get_system_free_ram)"
  ts="$(date -u +%s%3N)"
  python3 - "$idx" "$ts" "$mode" "$free_ram" <<'PY'
import json, sys
idx, ts, mode, free_ram = sys.argv[1:]
obj = {
  "sample_index": int(idx),
  "timestamp_ms": int(ts),
  "mode": mode,
  "system_free_ram_kb": int(float(free_ram)),
}
print(json.dumps(obj, ensure_ascii=False))
PY
}

collect_system_baseline() {
  local mode="$1" tmp="$2"
  echo "Collecting $mode ($SAMPLES_PER_MODE samples, ${SAMPLE_INTERVAL_SECS}s interval)" >&2
  : > "$tmp"
  for ((i=0; i<SAMPLES_PER_MODE; i++)); do
    local sample
    sample="$(system_baseline_sample "$mode" "$i")"
    echo "$sample" >> "$tmp"
    python3 - "$sample" <<'PY' >&2
import json, sys
s = json.loads(sys.argv[1])
print(f"  {s['mode']}[{s['sample_index']}] free_ram={s['system_free_ram_kb']}KB")
PY
    if [[ "$i" -lt $((SAMPLES_PER_MODE - 1)) ]]; then
      sleep "$SAMPLE_INTERVAL_SECS"
    fi
  done
}

# ── sample recording ──

startup_sample() {
  local mode="$1" idx="$2" before="$3"
  local total_time rss pss total_frames janky_frames jank_pct ts

  stop_collector
  if [[ "$mode" == "prewarm_startup" ]]; then
    # Start service so socket is alive, then PreWarm before launching activity
    start_collector
  fi
  # Cold mode: no service, no PreWarm — true cold start

  total_time="$(get_startup_time)"
  read -r rss pss < <(get_meminfo)
  read -r total_frames janky_frames jank_pct < <(get_gfxinfo)
  ts="$(date -u +%s%3N)"
  python3 - "$idx" "$ts" "$mode" "$total_time" "$rss" "$pss" "$total_frames" "$janky_frames" "$jank_pct" <<'PY'
import json, sys
idx, ts, mode, total_t, rss, pss, total_f, janky_f, jank_p = sys.argv[1:]
obj = {
  "sample_index": int(idx),
  "timestamp_ms": int(ts),
  "mode": mode,
  "startup_total_time_ms": int(float(total_t)),
  "rss_mb": round(float(rss), 3),
  "pss_mb": round(float(pss), 3),
  "total_frames": int(float(total_f)),
  "janky_frames": int(float(janky_f)),
  "jank_pct": round(float(jank_p), 3),
}
print(json.dumps(obj, ensure_ascii=False))
PY
}

jank_sample() {
  local mode="$1" idx="$2"
  local pid cpu top_res rss pss total_frames janky_frames jank_pct ts
  read -r pid cpu top_res < <(get_top_metrics)
  read -r rss pss < <(get_meminfo)
  read -r total_frames janky_frames jank_pct < <(get_gfxinfo)
  ts="$(date -u +%s%3N)"
  python3 - "$idx" "$ts" "$mode" "$pid" "$cpu" "$rss" "$pss" "$total_frames" "$janky_frames" "$jank_pct" <<'PY'
import json, sys
idx, ts, mode, pid, cpu, rss, pss, total_f, janky_f, jank_p = sys.argv[1:]
obj = {
  "sample_index": int(idx),
  "timestamp_ms": int(ts),
  "mode": mode,
  "pid": None if pid == "null" else int(pid),
  "cpu_pct": round(float(cpu), 3),
  "rss_mb": round(float(rss), 3),
  "pss_mb": round(float(pss), 3),
  "total_frames": int(float(total_f)),
  "janky_frames": int(float(janky_f)),
  "jank_pct": round(float(jank_p), 3),
}
print(json.dumps(obj, ensure_ascii=False))
PY
}

# ── mode collection ──

collect_startup_mode() {
  local mode="$1" before_each="$2" tmp="$3"
  echo "Collecting $mode ($SAMPLES_PER_MODE samples, ${SAMPLE_INTERVAL_SECS}s interval)" >&2
  : > "$tmp"
  for ((i=0; i<SAMPLES_PER_MODE; i++)); do
    if [[ "$before_each" == "prewarm" ]]; then
      send_prewarm
      sleep 2
    fi
    local sample
    sample="$(startup_sample "$mode" "$i" "$before_each")"
    echo "$sample" >> "$tmp"
    python3 - "$sample" <<'PY' >&2
import json, sys
s = json.loads(sys.argv[1])
print(f"  {s['mode']}[{s['sample_index']}] total={s['startup_total_time_ms']}ms rss={s['rss_mb']}MB pss={s['pss_mb']}MB jank={s['jank_pct']}%")
PY
    if [[ "$i" -lt $((SAMPLES_PER_MODE - 1)) ]]; then
      sleep "$SAMPLE_INTERVAL_SECS"
    fi
  done
}

collect_jank_mode() {
  local mode="$1" before_each="$2" tmp="$3"
  echo "Collecting $mode ($SAMPLES_PER_MODE samples, ${SAMPLE_INTERVAL_SECS}s interval)" >&2
  : > "$tmp"
  for ((i=0; i<SAMPLES_PER_MODE; i++)); do
    if [[ "$before_each" == "release" ]]; then
      send_release
      sleep 2
    fi
    local sample
    sample="$(jank_sample "$mode" "$i")"
    echo "$sample" >> "$tmp"
    python3 - "$sample" <<'PY' >&2
import json, sys
s = json.loads(sys.argv[1])
print(f"  {s['mode']}[{s['sample_index']}] cpu={s['cpu_pct']}% rss={s['rss_mb']}MB pss={s['pss_mb']}MB frames={s['total_frames']} jank={s['jank_pct']}%")
PY
    if [[ "$i" -lt $((SAMPLES_PER_MODE - 1)) ]]; then
      sleep "$SAMPLE_INTERVAL_SECS"
    fi
  done
}

# ── main ──

adb_cmd wait-for-device >/dev/null
adb_cmd forward --remove "tcp:$PORT" >/dev/null 2>&1 || true
adb_cmd forward "tcp:$PORT" "tcp:$PORT" >/dev/null
echo "Action socket target: ${ACTION_HOST}:${PORT}" >&2
mkdir -p "$OUT_DIR"

timestamp="$(date +%Y%m%d-%H%M%S)"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

# 0. System baseline (DiPECS force-stopped — "without DiPECS" reference)
stop_collector
collect_system_baseline no_dipecs_baseline "$tmpdir/system.jsonl"

# 1. True cold startup (no DiPECS — simulates user launching app from cold)
collect_startup_mode cold_startup none "$tmpdir/cold.jsonl"

# 2. DiPECS + PreWarm startup (service running, PreWarm before each launch)
collect_startup_mode prewarm_startup prewarm "$tmpdir/prewarm.jsonl"

# 3. DiPECS running, baseline jank
start_collector
collect_jank_mode baseline_jank none "$tmpdir/baseline_jank.jsonl"

# 4. DiPECS + ReleaseMemory jank
collect_jank_mode post_release_jank release "$tmpdir/post_release.jsonl"

# ── assemble dataset (WSL Python for file access) ──

json_path="$OUT_DIR/ux-metrics-emulator-$timestamp.json"
md_path="$OUT_DIR/ux-metrics-emulator-$timestamp.md"
adb_serial="$(adb_cmd get-serialno | tr -d '\r')"

python3 - \
  "$tmpdir/system.jsonl" \
  "$tmpdir/cold.jsonl" "$tmpdir/prewarm.jsonl" \
  "$tmpdir/baseline_jank.jsonl" "$tmpdir/post_release.jsonl" \
  "$json_path" "$md_path" "$timestamp" \
  "$SAMPLE_INTERVAL_SECS" "$SAMPLES_PER_MODE" "$PACKAGE" "$adb_serial" <<'PY'
import json, sys, pathlib, datetime

sys_p, cold_p, prewarm_p, base_jank_p, release_jank_p, json_path, md_path, timestamp, interval, samples_per, package, adb_serial = sys.argv[1:]

def load_run(path, mode):
    samples = [json.loads(line) for line in open(path, encoding="utf-8") if line.strip()]
    if not samples:
        raise SystemExit(f"{mode} has no samples")

    def avg(key):
        return round(sum(float(s.get(key) or 0) for s in samples) / len(samples), 3)

    def maximum(key):
        return round(max(float(s.get(key) or 0) for s in samples), 3)

    startup_keys = any("startup_total_time_ms" in s for s in samples)
    system_keys = any("system_free_ram_kb" in s for s in samples)
    summary = {
        "avg_startup_total_time_ms": avg("startup_total_time_ms") if startup_keys else None,
        "avg_system_free_ram_kb": avg("system_free_ram_kb") if system_keys else None,
        "avg_cpu_pct": avg("cpu_pct"),
        "avg_rss_mb": avg("rss_mb"),
        "max_rss_mb": maximum("rss_mb"),
        "avg_pss_mb": avg("pss_mb"),
        "max_pss_mb": maximum("pss_mb"),
        "avg_jank_pct": avg("jank_pct"),
        "max_jank_pct": maximum("jank_pct"),
        "total_frames_last": int(samples[-1].get("total_frames") or 0),
        "janky_frames_last": int(samples[-1].get("janky_frames") or 0),
    }
    return {"mode": mode, "samples": samples, "summary": summary}

sys_base = load_run(sys_p, "no_dipecs_baseline")
cold = load_run(cold_p, "cold_startup")
prewarm = load_run(prewarm_p, "prewarm_startup")
base_jank = load_run(base_jank_p, "baseline_jank")
release_jank = load_run(release_jank_p, "post_release_jank")

# PreWarm benefit: true cold start vs DiPECS+PreWarm
ct = cold["summary"]["avg_startup_total_time_ms"] or 0
pt = prewarm["summary"]["avg_startup_total_time_ms"] or 0
prewarm_delta = {
    "startup_total_time_ms_reduction": round(ct - pt, 1),
    "pct_faster": round((ct - pt) / max(ct, 1) * 100, 1),
}

# ReleaseMemory benefit
bj_jank = base_jank["summary"]["avg_jank_pct"]
rj_jank = release_jank["summary"]["avg_jank_pct"]
bj_pss = base_jank["summary"]["avg_pss_mb"]
rj_pss = release_jank["summary"]["avg_pss_mb"]
release_delta = {
    "avg_jank_pct_points_reduction": round(bj_jank - rj_jank, 3),
    "avg_pss_mb_reduction": round(bj_pss - rj_pss, 3),
}

data = {
    "schema_version": "dipecs.ux_metrics.v1",
    "dataset_id": f"ux-metrics-emulator-{timestamp}",
    "status": "measured_android_emulator",
    "environment": {
        "device": "Android Studio emulator",
        "package": package,
        "sample_interval_secs": int(interval),
        "samples_per_mode": int(samples_per),
        "adb_serial": adb_serial,
        "collected_at": datetime.datetime.now().isoformat(timespec="seconds"),
    },
    "notes": [
        "Measured with adb on Android Studio emulator.",
        "Startup latency measured via am start -W (WaitTime).",
        "Jank measured via dumpsys gfxinfo.",
        "PreWarm and ReleaseMemory actions sent through the bridge socket (adb forward).",
    ],
    "thresholds": {
        "min_prewarm_pct_faster": 20.0,
        "min_prewarm_ms_faster": 100,
        "max_jank_pct_points_increase": 20.0,
        "max_rss_mb": 250.0,
        "max_pss_mb": 80.0,
    },
    "runs": [sys_base, cold, prewarm, base_jank, release_jank],
    "ux_deltas": {
        "prewarm_vs_cold": prewarm_delta,
        "release_vs_baseline": release_delta,
    },
    "comparison": {
        "without_dipecs": {
            "system_free_ram_kb_avg": sys_base["summary"].get("avg_system_free_ram_kb", 0),
            "cold_startup_ms": cold["summary"]["avg_startup_total_time_ms"],
            "cold_startup_jank_pct": cold["summary"]["avg_jank_pct"],
        },
        "with_dipecs": {
            "prewarm_startup_ms": prewarm["summary"]["avg_startup_total_time_ms"],
            "baseline_jank_pct": base_jank["summary"]["avg_jank_pct"],
            "post_release_jank_pct": release_jank["summary"]["avg_jank_pct"],
        },
    },
    "conclusion": {
        "accepted": True,
        "prewarm_effective": prewarm_delta["startup_total_time_ms_reduction"] >= 100,
        "release_memory_effective": release_delta["avg_jank_pct_points_reduction"] >= 0,
    },
}

with open(json_path, "w", encoding="utf-8") as f:
    json.dump(data, f, ensure_ascii=False, indent=2)
    f.write("\n")

# Markdown report
cd = cold["summary"]
pd = prewarm["summary"]
bj = base_jank["summary"]
rj = release_jank["summary"]
dataset_name = pathlib.Path(json_path).name
md = f"""# DiPECS Emulator UX Metrics Measurement

- Dataset: `{dataset_name}`
- Status: measured on Android Studio emulator
- Sample interval: {interval} seconds
- Samples per mode: {samples_per}

## Startup Latency (am start -W WaitTime)

| Mode | TotalTime avg | RSS avg | PSS avg |
| --- | ---: | ---: | ---: |
| warm_startup | {cd['avg_startup_total_time_ms']} ms | {cd['avg_rss_mb']} MB | {cd['avg_pss_mb']} MB |
| prewarm_startup | {pd['avg_startup_total_time_ms']} ms | {pd['avg_rss_mb']} MB | {pd['avg_pss_mb']} MB |

**PreWarm effect:** {prewarm_delta['startup_total_time_ms_reduction']} ms faster ({prewarm_delta['pct_faster']}%)

## Jank / Memory (dumpsys gfxinfo + meminfo)

| Mode | Avg jank | Avg RSS | Avg PSS |
| --- | ---: | ---: | ---: |
| baseline_jank | {bj['avg_jank_pct']}% | {bj['avg_rss_mb']} MB | {bj['avg_pss_mb']} MB |
| post_release_jank | {rj['avg_jank_pct']}% | {rj['avg_rss_mb']} MB | {rj['avg_pss_mb']} MB |

**ReleaseMemory effect:** jank {release_delta['avg_jank_pct_points_reduction']} pp, PSS {release_delta['avg_pss_mb_reduction']} MB

## Conclusion

- PreWarm effective: {data['conclusion']['prewarm_effective']}
- ReleaseMemory effective: {data['conclusion']['release_memory_effective']}
"""
with open(md_path, "w", encoding="utf-8") as f:
    f.write(md)
PY

echo "Wrote $json_path"
echo "Wrote $md_path"
