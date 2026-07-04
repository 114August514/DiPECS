#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
ADB="${ADB:-adb}"
PACKAGE="${PACKAGE:-com.dipecs.collector}"
MISS_ACTUAL_PACKAGE="${MISS_ACTUAL_PACKAGE:-com.android.settings}"
SAMPLES="${SAMPLES:-20}"
SAMPLE_INTERVAL_SECS="${SAMPLE_INTERVAL_SECS:-1}"
TOKEN="${TOKEN:-dipecs-dev-emulator-shared-token-00000000}"
PORT="${PORT:-46321}"
ACTION_HOST="${ACTION_HOST:-127.0.0.1}"
DELAY="${DELAY:-1.0}"
OUT_DIR="${OUT_DIR:-$REPO_ROOT/data/evaluation/next-app}"
SENDER="$REPO_ROOT/tests/scenarios/lib/action-forensic-sender.py"
LSAPP_REPORT="${LSAPP_REPORT:-$REPO_ROOT/data/evaluation/next-app/lsapp-standard.report.json}"

mkdir -p "$OUT_DIR"
timestamp="$(date +%Y%m%d-%H%M%S)"
raw_dir="$(mktemp -d)"
trap 'rm -rf "$raw_dir"' EXIT

adb_cmd() {
  "$ADB" "$@"
}

total_time_from() {
  sed -n 's/.*TotalTime:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1
}

start_control() {
  adb_cmd shell am start -n "$PACKAGE/.debug.DebugCollectorControlActivity" >/dev/null 2>&1 ||
    adb_cmd shell am start -n "$PACKAGE/.MainActivity" --ez auto_start true >/dev/null 2>&1 || true
  sleep 4
}

stop_pkg() {
  adb_cmd shell am force-stop "$1" >/dev/null 2>&1 || true
}

home_screen() {
  adb_cmd shell input keyevent HOME >/dev/null 2>&1 || true
}

send_prewarm() {
  local line latency_us
  line="$(python3 "$SENDER" "$ACTION_HOST" "$PORT" "$TOKEN" "$DELAY" PreWarmProcess own:warmup Immediate 2>&1)"
  latency_us="$(printf '%s' "$line" | python3 -c '
import json, re, sys
text = sys.stdin.read()
m = re.search(r"device=({.*?})", text)
if not m:
    raise SystemExit("send_prewarm missing device response")
try:
    data = json.loads(m.group(1))
except Exception as err:
    raise SystemExit(f"send_prewarm invalid device response: {err}")
if data.get("status") != "ok":
    raise SystemExit(f"send_prewarm bridge did not accept action: {data}")
latency_us = int(data.get("latency_us") or 0)
if latency_us <= 0:
    raise SystemExit("send_prewarm missing positive latency_us")
print(latency_us)
')"
  printf '%s\t%s\n' "$latency_us" "$line"
}

measure_collector_start() {
  adb_cmd shell am start -W -n "$PACKAGE/.MainActivity" 2>/dev/null | total_time_from
}

measure_miss_actual_start() {
  if [[ "$MISS_ACTUAL_PACKAGE" == "com.android.settings" ]]; then
    adb_cmd shell am start -W -a android.settings.SETTINGS 2>/dev/null | total_time_from
  else
    adb_cmd shell monkey -p "$MISS_ACTUAL_PACKAGE" 1 2>/dev/null >/dev/null || true
    adb_cmd shell am start -W "$(adb_cmd shell cmd package resolve-activity --brief "$MISS_ACTUAL_PACKAGE" 2>/dev/null | tail -n 1 | tr -d '\r')" 2>/dev/null | total_time_from
  fi
}

write_startup_sample() {
  local file="$1" idx="$2" mode="$3" total="$4" prewarm_latency_us="${5:-0}"
  local ts
  if ! [[ "$total" =~ ^[0-9]+$ ]] || (( total <= 0 )); then
    echo "$mode startup TotalTime missing or non-positive: ${total:-<missing>}" >&2
    return 1
  fi
  if ! [[ "$prewarm_latency_us" =~ ^[0-9]+$ ]]; then
    echo "$mode prewarm latency missing or non-numeric: ${prewarm_latency_us:-<missing>}" >&2
    return 1
  fi
  ts="$(date -u +%s%3N)"
  printf '{"sample_index":%d,"timestamp_ms":%d,"mode":"%s","startup_total_time_ms":%d,"prewarm_latency_us":%d}\n' \
    "$idx" "$ts" "$mode" "$total" "$prewarm_latency_us" >> "$file"
}

collect_collector_cold() {
  local file="$raw_dir/collector_cold.jsonl"
  : > "$file"
  for ((i=0; i<SAMPLES; i++)); do
    stop_pkg "$PACKAGE"
    sleep 2
    local total
    total="$(measure_collector_start)"
    write_startup_sample "$file" "$i" collector_cold_startup "$total" 0
    echo "collector_cold_startup[$i] total=${total}ms" >&2
    home_screen
    sleep "$SAMPLE_INTERVAL_SECS"
  done
}

collect_collector_prewarm_hit() {
  local file="$raw_dir/collector_prewarm_hit.jsonl"
  : > "$file"
  for ((i=0; i<SAMPLES; i++)); do
    stop_pkg "$PACKAGE"
    sleep 2
    start_control
    local prewarm_latency line total
    read -r prewarm_latency line < <(send_prewarm)
    sleep 1
    total="$(measure_collector_start)"
    write_startup_sample "$file" "$i" collector_prewarm_hit_startup "$total" "$prewarm_latency"
    echo "collector_prewarm_hit_startup[$i] total=${total}ms prewarm_latency=${prewarm_latency}us" >&2
    home_screen
    sleep "$SAMPLE_INTERVAL_SECS"
  done
}

collect_miss_actual_cold() {
  local file="$raw_dir/miss_actual_cold.jsonl"
  : > "$file"
  for ((i=0; i<SAMPLES; i++)); do
    stop_pkg "$PACKAGE"
    stop_pkg "$MISS_ACTUAL_PACKAGE"
    sleep 2
    local total
    total="$(measure_miss_actual_start)"
    write_startup_sample "$file" "$i" settings_cold_startup "$total" 0
    echo "settings_cold_startup[$i] total=${total}ms" >&2
    home_screen
    sleep "$SAMPLE_INTERVAL_SECS"
  done
}

collect_miss_actual_after_wrong_prewarm() {
  local file="$raw_dir/miss_actual_after_wrong_prewarm.jsonl"
  : > "$file"
  for ((i=0; i<SAMPLES; i++)); do
    stop_pkg "$PACKAGE"
    stop_pkg "$MISS_ACTUAL_PACKAGE"
    sleep 2
    start_control
    local prewarm_latency line total
    read -r prewarm_latency line < <(send_prewarm)
    sleep 1
    stop_pkg "$MISS_ACTUAL_PACKAGE"
    total="$(measure_miss_actual_start)"
    write_startup_sample "$file" "$i" settings_after_wrong_prewarm_startup "$total" "$prewarm_latency"
    echo "settings_after_wrong_prewarm_startup[$i] total=${total}ms prewarm_latency=${prewarm_latency}us" >&2
    home_screen
    sleep "$SAMPLE_INTERVAL_SECS"
  done
}

assemble_report() {
  local json_path="$OUT_DIR/prewarm-net-benefit-real-device-$timestamp.json"
  local md_path="$OUT_DIR/prewarm-net-benefit-real-device-$timestamp.md"
  local serial
  serial="$(adb_cmd get-serialno | tr -d '\r')"
  python3 - \
    "$raw_dir/collector_cold.jsonl" \
    "$raw_dir/collector_prewarm_hit.jsonl" \
    "$raw_dir/miss_actual_cold.jsonl" \
    "$raw_dir/miss_actual_after_wrong_prewarm.jsonl" \
    "$LSAPP_REPORT" \
    "$json_path" "$md_path" "$timestamp" "$serial" "$PACKAGE" "$MISS_ACTUAL_PACKAGE" "$REPO_ROOT" <<'PY'
import datetime
import json
import math
import pathlib
import statistics
import sys

(
    collector_cold_p,
    collector_hit_p,
    miss_cold_p,
    miss_after_wrong_p,
    lsapp_p,
    json_path,
    md_path,
    timestamp,
    serial,
    package,
    miss_actual_package,
    repo_root,
) = sys.argv[1:]

repo_root_path = pathlib.Path(repo_root).resolve()

def rel(path):
    resolved = pathlib.Path(path).resolve()
    try:
        return resolved.relative_to(repo_root_path).as_posix()
    except ValueError:
        return resolved.as_posix()

def load(path, mode, require_prewarm_latency=False):
    samples = [json.loads(line) for line in open(path, encoding="utf-8") if line.strip()]
    if not samples:
        raise SystemExit(f"{mode} has no samples")
    values = [float(s["startup_total_time_ms"]) for s in samples]
    if any(value <= 0 for value in values):
        raise SystemExit(f"{mode} startup TotalTime missing or non-positive")
    latencies = [float(s.get("prewarm_latency_us") or 0) for s in samples]
    if require_prewarm_latency and (
        len(latencies) != len(samples) or any(latency <= 0 for latency in latencies)
    ):
        raise SystemExit(f"{mode} prewarm latency missing or non-positive")
    def percentile(pct):
        vals = sorted(values)
        rank = int(math.ceil(pct / 100.0 * len(vals)))
        return round(vals[max(0, min(rank - 1, len(vals) - 1))], 3)
    summary = {
        "n": len(samples),
        "mean_startup_total_time_ms": round(statistics.mean(values), 3),
        "p95_startup_total_time_ms": percentile(95.0),
        "mean_prewarm_latency_us": round(statistics.mean(latencies), 3)
        if require_prewarm_latency
        else 0.0,
    }
    return {"mode": mode, "samples": samples, "summary": summary}

collector_cold = load(collector_cold_p, "collector_cold_startup")
collector_hit = load(
    collector_hit_p,
    "collector_prewarm_hit_startup",
    require_prewarm_latency=True,
)
miss_cold = load(miss_cold_p, "settings_cold_startup")
miss_after_wrong = load(
    miss_after_wrong_p,
    "settings_after_wrong_prewarm_startup",
    require_prewarm_latency=True,
)

lsapp = json.load(open(lsapp_p, encoding="utf-8"))
examples = int(lsapp["test_examples"])
ensemble_hit = float(lsapp["metrics"]["ensemble"]["hit_rate_at_1_pct"])
strong_hit = float(lsapp["metrics"]["strong_predictive"]["hit_rate_at_1_pct"])

hit_saved_ms = (
    collector_cold["summary"]["mean_startup_total_time_ms"]
    - collector_hit["summary"]["mean_startup_total_time_ms"]
)
miss_startup_delta_ms = (
    miss_after_wrong["summary"]["mean_startup_total_time_ms"]
    - miss_cold["summary"]["mean_startup_total_time_ms"]
)
mean_prewarm_latency_ms = collector_hit["summary"]["mean_prewarm_latency_us"] / 1000.0
miss_action_cost_ms = max(0.0, miss_startup_delta_ms)
control_plane_ms = mean_prewarm_latency_ms

def benefit(hit_rate_pct):
    hit = hit_rate_pct / 100.0
    gross_saved = examples * hit * hit_saved_ms
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

ensemble = benefit(ensemble_hit)
strong = benefit(strong_hit)
n_at_least_20_per_mode = all(
    run["summary"]["n"] >= 20
    for run in [collector_cold, collector_hit, miss_cold, miss_after_wrong]
)
measured_inputs_valid = (
    math.isfinite(hit_saved_ms)
    and math.isfinite(miss_startup_delta_ms)
    and math.isfinite(mean_prewarm_latency_ms)
    and math.isfinite(miss_action_cost_ms)
    and math.isfinite(control_plane_ms)
    and hit_saved_ms > 0
    and mean_prewarm_latency_ms > 0
    and miss_action_cost_ms >= 0
    and control_plane_ms > 0
)
net_benefit_positive = ensemble["net_benefit_ms"] > 0
dipecs_beats_strong_predictive = ensemble["net_benefit_ms"] > strong["net_benefit_ms"]
accepted = (
    n_at_least_20_per_mode
    and measured_inputs_valid
    and net_benefit_positive
    and dipecs_beats_strong_predictive
)

data = {
    "schema_version": "dipecs.prewarm_net_benefit.v1",
    "dataset_id": f"prewarm-net-benefit-real-device-{timestamp}",
    "source": "measured_device",
    "status": "measured_android_real_device",
    "environment": {
        "device": "Android adb target",
        "package": package,
        "miss_actual_package": miss_actual_package,
        "adb_serial": serial,
        "samples_per_mode": collector_cold["summary"]["n"],
        "collected_at": datetime.datetime.now().isoformat(timespec="seconds"),
    },
    "provenance": {
        "prediction_report": rel(lsapp_p),
        "startup_measurement": "adb shell am start -W TotalTime",
        "hit_definition": "PreWarmProcess own:warmup before launching com.dipecs.collector",
        "miss_definition": "PreWarmProcess own:warmup before launching a different app",
    },
    "runs": [collector_cold, collector_hit, miss_cold, miss_after_wrong],
    "measured_inputs": {
        "source": "measured_device",
        "hit_saved_ms": round(hit_saved_ms, 3),
        "miss_startup_delta_ms": round(miss_startup_delta_ms, 3),
        "mean_prewarm_latency_ms": round(mean_prewarm_latency_ms, 3),
        "miss_action_cost_ms": round(miss_action_cost_ms, 3),
        "control_plane_ms": round(control_plane_ms, 3),
    },
    "net_benefit": {
        "source": "measured_device",
        "examples": examples,
        "action_budget": "top1_one_prewarm_per_lsapp_test_example",
        "dipecs_ensemble": ensemble,
        "strong_predictive": strong,
        "dipecs_minus_strong_net_benefit_ms": round(
            ensemble["net_benefit_ms"] - strong["net_benefit_ms"],
            3,
        ),
    },
    "conclusion": {
        "accepted": accepted,
        "n_at_least_20_per_mode": n_at_least_20_per_mode,
        "measured_inputs_valid": measured_inputs_valid,
        "net_benefit_positive": net_benefit_positive,
        "dipecs_beats_strong_predictive": dipecs_beats_strong_predictive,
    },
}

with open(json_path, "w", encoding="utf-8") as f:
    json.dump(data, f, ensure_ascii=False, indent=2)
    f.write("\n")

md = f"""# DiPECS PreWarm Net-Benefit Measurement

- Dataset: `{pathlib.Path(json_path).name}`
- Status: measured on Android adb target
- Samples per mode: {data['environment']['samples_per_mode']}
- Prediction report: `{data['provenance']['prediction_report']}`

## Startup Measurements

| Mode | Mean TotalTime | p95 TotalTime |
| --- | ---: | ---: |
| collector cold | {collector_cold['summary']['mean_startup_total_time_ms']} ms | {collector_cold['summary']['p95_startup_total_time_ms']} ms |
| collector after PreWarm hit | {collector_hit['summary']['mean_startup_total_time_ms']} ms | {collector_hit['summary']['p95_startup_total_time_ms']} ms |
| miss actual cold | {miss_cold['summary']['mean_startup_total_time_ms']} ms | {miss_cold['summary']['p95_startup_total_time_ms']} ms |
| miss actual after wrong PreWarm | {miss_after_wrong['summary']['mean_startup_total_time_ms']} ms | {miss_after_wrong['summary']['p95_startup_total_time_ms']} ms |

## Measured Inputs

- Hit saved latency: {data['measured_inputs']['hit_saved_ms']} ms
- Miss startup delta: {data['measured_inputs']['miss_startup_delta_ms']} ms
- Mean PreWarm dispatch latency: {data['measured_inputs']['mean_prewarm_latency_ms']} ms
- Miss action cost: {data['measured_inputs']['miss_action_cost_ms']} ms
- Control-plane / dispatch cost: {data['measured_inputs']['control_plane_ms']} ms per action

## Net Benefit

| Ranker | hit@1 | gross saved | gross wasted | control cost | net benefit |
| --- | ---: | ---: | ---: | ---: | ---: |
| DiPECS ensemble | {ensemble['hit_rate_at_1_pct']}% | {ensemble['gross_saved_ms']} ms | {ensemble['gross_wasted_ms']} ms | {ensemble['control_plane_cost_ms']} ms | {ensemble['net_benefit_ms']} ms |
| StrongPredictiveActionBaseline | {strong['hit_rate_at_1_pct']}% | {strong['gross_saved_ms']} ms | {strong['gross_wasted_ms']} ms | {strong['control_plane_cost_ms']} ms | {strong['net_benefit_ms']} ms |

DiPECS minus strong baseline: {data['net_benefit']['dipecs_minus_strong_net_benefit_ms']} ms.

## Scope

This artifact covers the #90 standard-split gate for Android-safe `PreWarmProcess own:*` evidence: LSApp standard hit@1 is reused from the committed prediction report, while hit/miss startup deltas and dispatch cost are measured on an adb target with n>=20 per mode. It does not claim silent third-party app prewarm on normal Android installs.
"""
with open(md_path, "w", encoding="utf-8") as f:
    f.write(md)

print(json_path)
print(md_path)
PY
}

adb_cmd wait-for-device >/dev/null
adb_cmd forward --remove "tcp:$PORT" >/dev/null 2>&1 || true
adb_cmd forward "tcp:$PORT" "tcp:$PORT" >/dev/null

collect_collector_cold
collect_collector_prewarm_hit
collect_miss_actual_cold
collect_miss_actual_after_wrong_prewarm
assemble_report
