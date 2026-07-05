#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
ADB="${ADB:-adb}"
PACKAGE="${PACKAGE:-com.dipecs.collector}"
SAMPLES="${SAMPLES:-20}"
SAMPLE_INTERVAL_SECS="${SAMPLE_INTERVAL_SECS:-1}"
PRESSURE_WINDOW_SECS="${PRESSURE_WINDOW_SECS:-12}"
PRESSURE_COMMAND="${PRESSURE_COMMAND:-}"
TOKEN="${TOKEN:-dipecs-dev-emulator-shared-token-00000000}"
PORT="${PORT:-46321}"
ACTION_HOST="${ACTION_HOST:-127.0.0.1}"
DELAY="${DELAY:-1.0}"
OUT_DIR="${OUT_DIR:-$REPO_ROOT/data/evaluation/action-net-benefit}"
SENDER="$REPO_ROOT/tests/scenarios/lib/action-forensic-sender.py"

# Optional same-budget comparison inputs. Leave unset to produce a pressure
# measurement artifact without accepting the #98 net-benefit gate.
EXAMPLES="${EXAMPLES:-0}"
DIPECS_HIT_RATE_PCT="${DIPECS_HIT_RATE_PCT:-}"
STRONG_HIT_RATE_PCT="${STRONG_HIT_RATE_PCT:-}"

mkdir -p "$OUT_DIR"
timestamp="$(date +%Y%m%d-%H%M%S)"
raw_dir="$(mktemp -d)"
trap 'rm -rf "$raw_dir"' EXIT

adb_cmd() {
  "$ADB" "$@"
}

start_control() {
  adb_cmd shell am start -n "$PACKAGE/.debug.DebugCollectorControlActivity" >/dev/null 2>&1 ||
    adb_cmd shell am start -n "$PACKAGE/.MainActivity" --ez auto_start true >/dev/null 2>&1 || true
  sleep 4
}

send_keepalive() {
  local line latency_us
  line="$(python3 "$SENDER" "$ACTION_HOST" "$PORT" "$TOKEN" "$DELAY" KeepAlive work:collector_heartbeat Immediate 2>&1)"
  latency_us="$(LINE="$line" python3 - <<'PY'
import json
import os
import re

text = os.environ["LINE"]
m = re.search(r"device=({.*?})", text)
if not m:
    raise SystemExit("KeepAlive missing device response")
try:
    data = json.loads(m.group(1))
except Exception as err:
    raise SystemExit(f"KeepAlive invalid device response: {err}")
if data.get("status") != "ok":
    raise SystemExit(f"KeepAlive bridge did not accept action: {data}")
latency_us = int(data.get("latency_us") or 0)
if latency_us <= 0:
    raise SystemExit("KeepAlive missing positive latency_us")
print(latency_us)
PY
)"
  printf '%s\t%s\n' "$latency_us" "$line"
}

pidof_package() {
  adb_cmd shell pidof "$PACKAGE" 2>/dev/null | tr -d '\r' | awk '{print $1}'
}

mem_available_kb() {
  adb_cmd shell cat /proc/meminfo 2>/dev/null |
    tr -d '\r' |
    awk '/^MemAvailable:/ {print $2; found=1} END {if (!found) print 0}'
}

pss_kb() {
  local pid="$1"
  if [[ -z "$pid" || "$pid" == "0" ]]; then
    printf '0\n'
    return
  fi
  adb_cmd shell dumpsys meminfo "$pid" 2>/dev/null |
    tr -d '\r' |
    awk '
      /TOTAL PSS:/ {print $3; found=1; exit}
      /^ *TOTAL +[0-9]+/ {print $2; found=1; exit}
      END {if (!found) print 0}
    '
}

jank_pct() {
  adb_cmd shell dumpsys gfxinfo "$PACKAGE" 2>/dev/null |
    tr -d '\r' |
    awk '
      /Total frames rendered:/ {total=$4}
      /Janky frames:/ {
        janky=$3
        gsub(/\(.*/, "", janky)
      }
      END {
        if (total > 0) {
          printf "%.3f\n", (janky * 100.0 / total)
        } else {
          print "0.000"
        }
      }
    '
}

reset_gfxinfo() {
  adb_cmd shell dumpsys gfxinfo "$PACKAGE" reset >/dev/null 2>&1 || true
}

run_pressure_window() {
  if [[ -z "$PRESSURE_COMMAND" ]]; then
    echo "PRESSURE_COMMAND is required for #98 evidence" >&2
    exit 1
  fi
  # The pressure command should block for roughly PRESSURE_WINDOW_SECS and may
  # use $ADB/$PACKAGE from the environment. Example:
  # PRESSURE_COMMAND='adb shell am start -n com.example.pressure/.MainActivity --ei mb 512 --ei seconds 12'
  export ADB PACKAGE PRESSURE_WINDOW_SECS
  bash -lc "$PRESSURE_COMMAND" &
  local pressure_pid=$!
  sleep "$PRESSURE_WINDOW_SECS"
  wait "$pressure_pid" || {
    echo "PRESSURE_COMMAND failed" >&2
    return 1
  }
}

write_sample() {
  local file="$1"
  local idx="$2"
  local mode="$3"
  local keepalive_latency_us="$4"
  local pid_before="$5"
  local pid_after="$6"
  local mem_before="$7"
  local mem_after="$8"
  local pss_before="$9"
  local pss_after="${10}"
  local jank="${11}"
  local ts survived pid_changed restart_count mem_drop pss_delta
  ts="$(date -u +%s%3N)"
  survived=false
  pid_changed=true
  restart_count=1
  if [[ -n "$pid_after" && "$pid_after" != "0" ]]; then
    survived=true
    if [[ "$pid_before" == "$pid_after" ]]; then
      pid_changed=false
      restart_count=0
    fi
  fi
  mem_drop="$(python3 - "$mem_before" "$mem_after" <<'PY'
import sys
print(int(float(sys.argv[1])) - int(float(sys.argv[2])))
PY
)"
  pss_delta="$(python3 - "$pss_before" "$pss_after" <<'PY'
import sys
print(int(float(sys.argv[2])) - int(float(sys.argv[1])))
PY
)"
  printf '{"sample_index":%d,"timestamp_ms":%d,"mode":"%s","keepalive_latency_us":%d,"pid_before":"%s","pid_after":"%s","survived":%s,"pid_changed":%s,"restart_count":%d,"mem_available_before_kb":%d,"mem_available_after_kb":%d,"mem_available_drop_kb":%d,"pss_before_kb":%d,"pss_after_kb":%d,"pss_delta_kb":%d,"jank_pct":%.3f}\n' \
    "$idx" "$ts" "$mode" "$keepalive_latency_us" "$pid_before" "$pid_after" \
    "$survived" "$pid_changed" "$restart_count" "$mem_before" "$mem_after" \
    "$mem_drop" "$pss_before" "$pss_after" "$pss_delta" "$jank" >> "$file"
}

collect_mode() {
  local mode="$1"
  local with_keepalive="$2"
  local file="$raw_dir/$mode.jsonl"
  : > "$file"
  for ((i=0; i<SAMPLES; i++)); do
    adb_cmd shell am force-stop "$PACKAGE" >/dev/null 2>&1 || true
    start_control
    reset_gfxinfo
    local pid_before mem_before pss_before keepalive_latency pid_after mem_after pss_after jank
    pid_before="$(pidof_package)"
    mem_before="$(mem_available_kb)"
    pss_before="$(pss_kb "$pid_before")"
    keepalive_latency=0
    if [[ "$with_keepalive" == "true" ]]; then
      read -r keepalive_latency _ < <(send_keepalive)
    fi
    run_pressure_window
    pid_after="$(pidof_package)"
    mem_after="$(mem_available_kb)"
    pss_after="$(pss_kb "$pid_after")"
    jank="$(jank_pct)"
    write_sample "$file" "$i" "$mode" "$keepalive_latency" "$pid_before" "$pid_after" \
      "$mem_before" "$mem_after" "$pss_before" "$pss_after" "$jank"
    echo "$mode[$i] survived=$( [[ -n "$pid_after" ]] && echo true || echo false ) pid_before=$pid_before pid_after=$pid_after mem_drop=$((mem_before - mem_after))KB jank=$jank%" >&2
    sleep "$SAMPLE_INTERVAL_SECS"
  done
}

assemble_report() {
  local json_path="$OUT_DIR/keepalive-pressure-benefit-$timestamp.json"
  local md_path="$OUT_DIR/keepalive-pressure-benefit-$timestamp.md"
  local serial
  serial="$(adb_cmd get-serialno | tr -d '\r')"
  python3 - \
    "$raw_dir/baseline_pressure.jsonl" \
    "$raw_dir/keepalive_pressure.jsonl" \
    "$json_path" "$md_path" "$timestamp" "$serial" "$PACKAGE" "$EXAMPLES" \
    "${DIPECS_HIT_RATE_PCT:-}" "${STRONG_HIT_RATE_PCT:-}" "$REPO_ROOT" <<'PY'
import datetime
import json
import math
import pathlib
import statistics
import sys

(
    baseline_p,
    keepalive_p,
    json_path,
    md_path,
    timestamp,
    serial,
    package,
    examples_s,
    dipecs_hit_s,
    strong_hit_s,
    repo_root,
) = sys.argv[1:]

repo_root_path = pathlib.Path(repo_root).resolve()

def rel(path):
    resolved = pathlib.Path(path).resolve()
    try:
        return resolved.relative_to(repo_root_path).as_posix()
    except ValueError:
        return resolved.as_posix()

def percentile(values, pct):
    vals = sorted(values)
    rank = int(math.ceil(pct / 100.0 * len(vals)))
    return vals[max(0, min(rank - 1, len(vals) - 1))]

def load(path, mode):
    samples = [json.loads(line) for line in open(path, encoding="utf-8") if line.strip()]
    if not samples:
        raise SystemExit(f"{mode} has no samples")
    survived = [1 if s["survived"] else 0 for s in samples]
    restarts = [int(s["restart_count"]) for s in samples]
    mem_drop = [float(s["mem_available_drop_kb"]) for s in samples]
    pss_delta = [float(s["pss_delta_kb"]) for s in samples]
    jank = [float(s["jank_pct"]) for s in samples]
    latencies = [float(s.get("keepalive_latency_us") or 0) for s in samples]
    return {
        "mode": mode,
        "samples": samples,
        "summary": {
            "n": len(samples),
            "survival_rate_pct": round(100.0 * statistics.mean(survived), 3),
            "restart_count_mean": round(statistics.mean(restarts), 3),
            "restart_count_p95": round(percentile(restarts, 95.0), 3),
            "mean_mem_available_drop_kb": round(statistics.mean(mem_drop), 3),
            "mean_pss_delta_kb": round(statistics.mean(pss_delta), 3),
            "mean_jank_pct": round(statistics.mean(jank), 3),
            "p95_jank_pct": round(percentile(jank, 95.0), 3),
            "mean_keepalive_latency_us": round(statistics.mean(latencies), 3),
        },
    }

baseline = load(baseline_p, "baseline_pressure")
keepalive = load(keepalive_p, "keepalive_pressure")

survival_lift_pct_points = (
    keepalive["summary"]["survival_rate_pct"] - baseline["summary"]["survival_rate_pct"]
)
restart_reduction = (
    baseline["summary"]["restart_count_mean"] - keepalive["summary"]["restart_count_mean"]
)
jank_delta_pct_points = keepalive["summary"]["mean_jank_pct"] - baseline["summary"]["mean_jank_pct"]
pss_delta_over_baseline_kb = (
    keepalive["summary"]["mean_pss_delta_kb"] - baseline["summary"]["mean_pss_delta_kb"]
)
control_plane_ms = keepalive["summary"]["mean_keepalive_latency_us"] / 1000.0

def parse_optional_float(value):
    return float(value) if value.strip() else None

examples = int(examples_s or "0")
dipecs_hit = parse_optional_float(dipecs_hit_s)
strong_hit = parse_optional_float(strong_hit_s)
has_baseline_inputs = examples > 0 and dipecs_hit is not None and strong_hit is not None

saved_restart_cost_ms = 1000.0
resource_penalty_ms = max(0.0, jank_delta_pct_points) * 100.0 + max(0.0, pss_delta_over_baseline_kb) / 1024.0
measured_saved_ms = max(0.0, restart_reduction) * saved_restart_cost_ms
miss_action_cost_ms = resource_penalty_ms

def benefit(hit_rate_pct):
    hit = hit_rate_pct / 100.0
    gross_saved = examples * hit * measured_saved_ms
    gross_wasted = examples * (1.0 - hit) * miss_action_cost_ms
    control = examples * control_plane_ms
    return {
        "source": "measured_device",
        "hit_rate_at_1_pct": round(hit_rate_pct, 3),
        "gross_saved_ms": round(gross_saved, 3),
        "gross_wasted_ms": round(gross_wasted, 3),
        "control_plane_cost_ms": round(control, 3),
        "net_benefit_ms": round(gross_saved - gross_wasted - control, 3),
    }

dipecs = benefit(dipecs_hit) if has_baseline_inputs else None
strong = benefit(strong_hit) if has_baseline_inputs else None
n_at_least_20_per_mode = all(run["summary"]["n"] >= 20 for run in [baseline, keepalive])
memory_pressure_observed = (
    baseline["summary"]["mean_mem_available_drop_kb"] > 0
    or keepalive["summary"]["mean_mem_available_drop_kb"] > 0
)
measured_inputs_valid = (
    math.isfinite(survival_lift_pct_points)
    and math.isfinite(restart_reduction)
    and math.isfinite(control_plane_ms)
    and control_plane_ms > 0
)
net_benefit_positive = bool(dipecs and dipecs["net_benefit_ms"] > 0)
dipecs_beats_strong_predictive = bool(
    dipecs and strong and dipecs["net_benefit_ms"] > strong["net_benefit_ms"]
)
accepted = (
    n_at_least_20_per_mode
    and memory_pressure_observed
    and measured_inputs_valid
    and has_baseline_inputs
    and net_benefit_positive
    and dipecs_beats_strong_predictive
)

data = {
    "schema_version": "dipecs.keepalive_pressure_benefit.v1",
    "dataset_id": f"keepalive-pressure-benefit-{timestamp}",
    "action": "KeepAlive",
    "source": "measured_device",
    "status": "measured_android_device" if accepted else "measurement_pending_baseline_gate",
    "environment": {
        "device": "Android adb target",
        "adb_serial": serial,
        "package": package,
        "samples_per_mode": baseline["summary"]["n"],
        "pressure_window_secs": int(float(__import__("os").environ.get("PRESSURE_WINDOW_SECS", "12"))),
        "collected_at": datetime.datetime.now().isoformat(timespec="seconds"),
    },
    "provenance": {
        "measurement": "Compare collector process survival under an explicit memory pressure command with and without KeepAlive.",
        "pressure_command_required": True,
        "raw_baseline_samples": rel(baseline_p),
        "raw_keepalive_samples": rel(keepalive_p),
    },
    "runs": [baseline, keepalive],
    "measured_inputs": {
        "source": "measured_device",
        "survival_lift_pct_points": round(survival_lift_pct_points, 3),
        "restart_reduction_mean": round(restart_reduction, 3),
        "jank_delta_pct_points": round(jank_delta_pct_points, 3),
        "pss_delta_over_baseline_kb": round(pss_delta_over_baseline_kb, 3),
        "measured_saved_ms": round(measured_saved_ms, 3),
        "miss_action_cost_ms": round(miss_action_cost_ms, 3),
        "control_plane_ms": round(control_plane_ms, 3),
    },
    "net_benefit": {
        "source": "measured_device",
        "examples": examples,
        "action_budget": "top1_one_keepalive_per_test_example",
        "dipecs_ensemble": dipecs,
        "strong_predictive": strong,
        "dipecs_minus_strong_net_benefit_ms": round(
            (dipecs["net_benefit_ms"] - strong["net_benefit_ms"])
            if dipecs and strong else 0.0,
            3,
        ),
    },
    "conclusion": {
        "accepted": accepted,
        "n_at_least_20_per_mode": n_at_least_20_per_mode,
        "memory_pressure_observed": memory_pressure_observed,
        "measured_inputs_valid": measured_inputs_valid,
        "same_budget_baseline_inputs_present": has_baseline_inputs,
        "net_benefit_positive": net_benefit_positive,
        "dipecs_beats_strong_predictive": dipecs_beats_strong_predictive,
    },
}

with open(json_path, "w", encoding="utf-8") as f:
    json.dump(data, f, ensure_ascii=False, indent=2)
    f.write("\n")

md = f"""# DiPECS KeepAlive Pressure Benefit Measurement

- Dataset: `{pathlib.Path(json_path).name}`
- Status: {data['status']}
- Samples per mode: {baseline['summary']['n']}

## Pressure Survival

| Mode | Survival | Restart mean | p95 restarts | Mean mem drop | Mean PSS delta | Mean jank |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| baseline pressure | {baseline['summary']['survival_rate_pct']}% | {baseline['summary']['restart_count_mean']} | {baseline['summary']['restart_count_p95']} | {baseline['summary']['mean_mem_available_drop_kb']} KB | {baseline['summary']['mean_pss_delta_kb']} KB | {baseline['summary']['mean_jank_pct']}% |
| keepalive pressure | {keepalive['summary']['survival_rate_pct']}% | {keepalive['summary']['restart_count_mean']} | {keepalive['summary']['restart_count_p95']} | {keepalive['summary']['mean_mem_available_drop_kb']} KB | {keepalive['summary']['mean_pss_delta_kb']} KB | {keepalive['summary']['mean_jank_pct']}% |

## Measured Inputs

- Survival lift: {data['measured_inputs']['survival_lift_pct_points']} pp
- Restart reduction: {data['measured_inputs']['restart_reduction_mean']}
- Jank delta: {data['measured_inputs']['jank_delta_pct_points']} pp
- PSS delta over baseline: {data['measured_inputs']['pss_delta_over_baseline_kb']} KB
- Control-plane / dispatch cost: {data['measured_inputs']['control_plane_ms']} ms per action

## Acceptance

Same-budget comparison inputs present: {has_baseline_inputs}.
Accepted: {accepted}.

This artifact is accepted for #98 only when n>=20 per mode, memory pressure is observed, measured inputs are valid, same-budget hit-rate inputs are present for DiPECS and StrongPredictiveActionBaseline, DiPECS net benefit is positive, and DiPECS beats the strong baseline.
"""
with open(md_path, "w", encoding="utf-8") as f:
    f.write(md)

print(json_path)
print(md_path)
PY
}

if (( SAMPLES < 20 )); then
  echo "SAMPLES must be >=20 for #98 evidence; got $SAMPLES" >&2
  exit 1
fi
if [[ -z "$PRESSURE_COMMAND" ]]; then
  echo "PRESSURE_COMMAND is required for #98 evidence" >&2
  exit 1
fi

adb_cmd wait-for-device >/dev/null
adb_cmd forward --remove "tcp:$PORT" >/dev/null 2>&1 || true
adb_cmd forward "tcp:$PORT" "tcp:$PORT" >/dev/null

collect_mode baseline_pressure false
collect_mode keepalive_pressure true
assemble_report
