#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
ADB="${ADB:-adb}"
PACKAGE="${PACKAGE:-com.dipecs.collector}"
SAMPLES="${SAMPLES:-20}"
SAMPLE_INTERVAL_SECS="${SAMPLE_INTERVAL_SECS:-1}"
TOKEN="${TOKEN:-dipecs-dev-emulator-shared-token-00000000}"
PORT="${PORT:-46321}"
ACTION_HOST="${ACTION_HOST:-127.0.0.1}"
DELAY="${DELAY:-1.0}"
PREFETCH_URL="${PREFETCH_URL:-https://raw.githubusercontent.com/114August514/DiPECS/main/README.md}"
OUT_DIR="${OUT_DIR:-$REPO_ROOT/data/evaluation/action-net-benefit}"
SENDER="$REPO_ROOT/tests/scenarios/lib/action-forensic-sender.py"

# Optional same-budget comparison inputs. Leave unset to produce a measurement
# artifact without accepting the #97 net-benefit gate.
EXAMPLES="${EXAMPLES:-0}"
DIPECS_HIT_RATE_PCT="${DIPECS_HIT_RATE_PCT:-}"
STRONG_HIT_RATE_PCT="${STRONG_HIT_RATE_PCT:-}"

mkdir -p "$OUT_DIR"
timestamp="$(date +%Y%m%d-%H%M%S)"
raw_dir="$(mktemp -d)"
trap 'rm -rf "$raw_dir"' EXIT

TARGET="url:$PREFETCH_URL"

adb_cmd() {
  "$ADB" "$@"
}

cache_file_name() {
  python3 - "$PREFETCH_URL" <<'PY'
import hashlib
import sys

value = sys.argv[1]
digest = hashlib.sha256(value.encode("utf-8")).hexdigest()
tail = value.rsplit("/", 1)[-1]
ext = tail.rsplit(".", 1)[-1] if "." in tail else ""
if 1 <= len(ext) <= 8 and ext.isalnum():
    print(f"{digest}.{ext}")
else:
    print(digest)
PY
}

CACHE_FILE_NAME="$(cache_file_name)"
CACHE_PATH="/data/user/0/$PACKAGE/cache/prefetch/$CACHE_FILE_NAME"

start_control() {
  adb_cmd shell am start -n "$PACKAGE/.debug.DebugCollectorControlActivity" >/dev/null 2>&1 ||
    adb_cmd shell am start -n "$PACKAGE/.MainActivity" --ez auto_start true >/dev/null 2>&1 || true
  sleep 4
}

send_action() {
  local action_type="$1"
  local target="$2"
  local line latency_us
  line="$(python3 "$SENDER" "$ACTION_HOST" "$PORT" "$TOKEN" "$DELAY" "$action_type" "$target" Immediate 2>&1)"
  latency_us="$(LINE="$line" python3 - "$action_type" <<'PY'
import json
import os
import re
import sys

text = os.environ["LINE"]
action_type = sys.argv[1]
m = re.search(r"device=({.*?})", text)
if not m:
    raise SystemExit(f"{action_type} missing device response")
try:
    data = json.loads(m.group(1))
except Exception as err:
    raise SystemExit(f"{action_type} invalid device response: {err}")
if data.get("status") != "ok":
    raise SystemExit(f"{action_type} bridge did not accept action: {data}")
latency_us = int(data.get("latency_us") or 0)
if latency_us <= 0:
    raise SystemExit(f"{action_type} missing positive latency_us")
print(latency_us)
PY
)"
  printf '%s\t%s\n' "$latency_us" "$line"
}

clear_prefetch_cache() {
  local latency line
  read -r latency line < <(send_action ReleaseMemory cache:prefetch)
  printf '%s\n' "$latency"
}

send_prefetch() {
  local latency line
  read -r latency line < <(send_action PrefetchFile "$TARGET")
  printf '%s\n' "$latency"
}

cache_exists() {
  adb_cmd shell run-as "$PACKAGE" sh -c "test -s '$CACHE_PATH'" >/dev/null 2>&1
}

wait_cache_file() {
  local timeout_ms="${1:-30000}"
  local started now
  started="$(date +%s%3N)"
  while true; do
    if cache_exists; then
      now="$(date +%s%3N)"
      printf '%s\n' "$((now - started))"
      return 0
    fi
    now="$(date +%s%3N)"
    if (( now - started > timeout_ms )); then
      echo "PrefetchFile did not create expected cache file: $CACHE_PATH" >&2
      return 1
    fi
    sleep 0.25
  done
}

read_cache_once_ms() {
  if ! cache_exists; then
    echo "cache file missing before read: $CACHE_PATH" >&2
    return 1
  fi
  python3 - "$ADB" "$PACKAGE" "$CACHE_PATH" <<'PY'
import subprocess
import sys
import time

adb, package, cache_path = sys.argv[1:]
started = time.perf_counter_ns()
result = subprocess.run(
    [
        adb,
        "shell",
        "run-as",
        package,
        "sh",
        "-c",
        f"cat {cache_path!r} > /dev/null",
    ],
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
)
elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000.0
if result.returncode != 0:
    raise SystemExit(
        f"cache read failed: rc={result.returncode} stderr={result.stderr.strip()}"
    )
print(f"{elapsed_ms:.3f}")
PY
}

cache_size_bytes() {
  adb_cmd shell run-as "$PACKAGE" sh -c "stat -c %s '$CACHE_PATH' 2>/dev/null || wc -c < '$CACHE_PATH'" |
    tr -d '\r[:space:]'
}

write_sample() {
  local file="$1"
  local idx="$2"
  local mode="$3"
  local clear_latency_us="$4"
  local prefetch_latency_us="$5"
  local prefetch_wait_ms="$6"
  local read_ms="$7"
  local total_ms="$8"
  local bytes="$9"
  local ts
  for value in "$read_ms" "$total_ms"; do
    python3 - "$value" <<'PY'
import math
import sys
value = float(sys.argv[1])
if not math.isfinite(value) or value <= 0:
    raise SystemExit("PrefetchFile measurement missing or non-positive")
PY
  done
  ts="$(date -u +%s%3N)"
  printf '{"sample_index":%d,"timestamp_ms":%d,"mode":"%s","target":"%s","cache_path":"%s","cache_bytes":%d,"clear_latency_us":%d,"prefetch_latency_us":%d,"prefetch_wait_ms":%.3f,"read_ms":%.3f,"total_ms":%.3f}\n' \
    "$idx" "$ts" "$mode" "$TARGET" "$CACHE_PATH" "$bytes" "$clear_latency_us" \
    "$prefetch_latency_us" "$prefetch_wait_ms" "$read_ms" "$total_ms" >> "$file"
}

collect_prefetched_read() {
  local file="$raw_dir/prefetched_read.jsonl"
  : > "$file"
  for ((i=0; i<SAMPLES; i++)); do
    local clear_latency prefetch_latency prefetch_wait read_ms bytes
    clear_latency="$(clear_prefetch_cache)"
    prefetch_latency="$(send_prefetch)"
    prefetch_wait="$(wait_cache_file 30000)"
    read_ms="$(read_cache_once_ms)"
    bytes="$(cache_size_bytes)"
    write_sample "$file" "$i" prefetched_read "$clear_latency" "$prefetch_latency" \
      "$prefetch_wait" "$read_ms" "$read_ms" "$bytes"
    echo "prefetched_read[$i] read=${read_ms}ms wait=${prefetch_wait}ms bytes=${bytes}" >&2
    sleep "$SAMPLE_INTERVAL_SECS"
  done
}

collect_miss_fetch_then_read() {
  local file="$raw_dir/miss_fetch_then_read.jsonl"
  : > "$file"
  for ((i=0; i<SAMPLES; i++)); do
    local clear_latency prefetch_latency prefetch_wait read_ms total_ms bytes
    clear_latency="$(clear_prefetch_cache)"
    prefetch_latency="$(send_prefetch)"
    prefetch_wait="$(wait_cache_file 30000)"
    read_ms="$(read_cache_once_ms)"
    total_ms="$(python3 - "$prefetch_wait" "$read_ms" <<'PY'
import sys
print(f"{float(sys.argv[1]) + float(sys.argv[2]):.3f}")
PY
)"
    bytes="$(cache_size_bytes)"
    write_sample "$file" "$i" miss_fetch_then_read "$clear_latency" "$prefetch_latency" \
      "$prefetch_wait" "$read_ms" "$total_ms" "$bytes"
    echo "miss_fetch_then_read[$i] total=${total_ms}ms wait=${prefetch_wait}ms read=${read_ms}ms bytes=${bytes}" >&2
    sleep "$SAMPLE_INTERVAL_SECS"
  done
}

assemble_report() {
  local json_path="$OUT_DIR/prefetch-file-benefit-$timestamp.json"
  local md_path="$OUT_DIR/prefetch-file-benefit-$timestamp.md"
  local serial
  serial="$(adb_cmd get-serialno | tr -d '\r')"
  python3 - \
    "$raw_dir/prefetched_read.jsonl" \
    "$raw_dir/miss_fetch_then_read.jsonl" \
    "$json_path" "$md_path" "$timestamp" "$serial" "$PACKAGE" "$TARGET" \
    "$EXAMPLES" "${DIPECS_HIT_RATE_PCT:-}" "${STRONG_HIT_RATE_PCT:-}" "$REPO_ROOT" <<'PY'
import datetime
import json
import math
import pathlib
import statistics
import sys

(
    prefetched_p,
    miss_p,
    json_path,
    md_path,
    timestamp,
    serial,
    package,
    target,
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
    totals = [float(s["total_ms"]) for s in samples]
    reads = [float(s["read_ms"]) for s in samples]
    waits = [float(s["prefetch_wait_ms"]) for s in samples]
    bytes_values = [int(s["cache_bytes"]) for s in samples]
    if any(value <= 0 for value in totals + reads):
        raise SystemExit(f"{mode} contains non-positive timing")
    if any(value <= 0 for value in bytes_values):
        raise SystemExit(f"{mode} contains an empty cache file")
    return {
        "mode": mode,
        "samples": samples,
        "summary": {
            "n": len(samples),
            "mean_total_ms": round(statistics.mean(totals), 3),
            "p95_total_ms": round(percentile(totals, 95.0), 3),
            "mean_read_ms": round(statistics.mean(reads), 3),
            "p95_read_ms": round(percentile(reads, 95.0), 3),
            "mean_prefetch_wait_ms": round(statistics.mean(waits), 3),
            "p95_prefetch_wait_ms": round(percentile(waits, 95.0), 3),
            "mean_cache_bytes": round(statistics.mean(bytes_values), 3),
        },
    }

prefetched = load(prefetched_p, "prefetched_read")
miss = load(miss_p, "miss_fetch_then_read")
hit_saved_ms = miss["summary"]["mean_total_ms"] - prefetched["summary"]["mean_read_ms"]
miss_cost_ms = miss["summary"]["mean_prefetch_wait_ms"]
control_plane_ms = statistics.mean(
    float(s["prefetch_latency_us"]) / 1000.0
    for s in prefetched["samples"] + miss["samples"]
)

def parse_optional_float(value):
    return float(value) if value.strip() else None

examples = int(examples_s or "0")
dipecs_hit = parse_optional_float(dipecs_hit_s)
strong_hit = parse_optional_float(strong_hit_s)
has_baseline_inputs = examples > 0 and dipecs_hit is not None and strong_hit is not None

def benefit(hit_rate_pct):
    hit = hit_rate_pct / 100.0
    gross_saved = examples * hit * hit_saved_ms
    gross_wasted = examples * (1.0 - hit) * miss_cost_ms
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
n_at_least_20_per_mode = all(run["summary"]["n"] >= 20 for run in [prefetched, miss])
measured_inputs_valid = (
    math.isfinite(hit_saved_ms)
    and math.isfinite(miss_cost_ms)
    and math.isfinite(control_plane_ms)
    and hit_saved_ms > 0
    and miss_cost_ms > 0
    and control_plane_ms > 0
)
net_benefit_positive = bool(dipecs and dipecs["net_benefit_ms"] > 0)
dipecs_beats_strong_predictive = bool(
    dipecs and strong and dipecs["net_benefit_ms"] > strong["net_benefit_ms"]
)
accepted = (
    n_at_least_20_per_mode
    and measured_inputs_valid
    and has_baseline_inputs
    and net_benefit_positive
    and dipecs_beats_strong_predictive
)

data = {
    "schema_version": "dipecs.prefetch_file_benefit.v1",
    "dataset_id": f"prefetch-file-benefit-{timestamp}",
    "action": "PrefetchFile",
    "source": "measured_device",
    "status": "measured_android_device" if accepted else "measurement_pending_baseline_gate",
    "environment": {
        "device": "Android adb target",
        "adb_serial": serial,
        "package": package,
        "target": target,
        "samples_per_mode": prefetched["summary"]["n"],
        "collected_at": datetime.datetime.now().isoformat(timespec="seconds"),
    },
    "provenance": {
        "measurement": "PrefetchFile HTTPS target into app cache, then run-as cat cache file to /dev/null",
        "cache_hit_mode": "prefetched_read measures first read after the PrefetchFile cache exists",
        "miss_mode": "miss_fetch_then_read clears cache, waits for PrefetchFile to recreate it, then reads it",
        "same_budget_baseline": "requires EXAMPLES, DIPECS_HIT_RATE_PCT, and STRONG_HIT_RATE_PCT",
        "raw_prefetched_samples": rel(prefetched_p),
        "raw_miss_samples": rel(miss_p),
    },
    "runs": [prefetched, miss],
    "measured_inputs": {
        "source": "measured_device",
        "hit_saved_ms": round(hit_saved_ms, 3),
        "miss_action_cost_ms": round(miss_cost_ms, 3),
        "control_plane_ms": round(control_plane_ms, 3),
    },
    "net_benefit": {
        "source": "measured_device",
        "examples": examples,
        "action_budget": "top1_one_prefetch_per_test_example",
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
        "measured_inputs_valid": measured_inputs_valid,
        "same_budget_baseline_inputs_present": has_baseline_inputs,
        "net_benefit_positive": net_benefit_positive,
        "dipecs_beats_strong_predictive": dipecs_beats_strong_predictive,
    },
}

with open(json_path, "w", encoding="utf-8") as f:
    json.dump(data, f, ensure_ascii=False, indent=2)
    f.write("\n")

md = f"""# DiPECS PrefetchFile Benefit Measurement

- Dataset: `{pathlib.Path(json_path).name}`
- Status: {data['status']}
- Target: `{target}`
- Samples per mode: {data['environment']['samples_per_mode']}

## Latency

| Mode | Mean total | p95 total | Mean read | p95 read | Mean prefetch wait |
| --- | ---: | ---: | ---: | ---: | ---: |
| prefetched read | {prefetched['summary']['mean_total_ms']} ms | {prefetched['summary']['p95_total_ms']} ms | {prefetched['summary']['mean_read_ms']} ms | {prefetched['summary']['p95_read_ms']} ms | {prefetched['summary']['mean_prefetch_wait_ms']} ms |
| miss fetch then read | {miss['summary']['mean_total_ms']} ms | {miss['summary']['p95_total_ms']} ms | {miss['summary']['mean_read_ms']} ms | {miss['summary']['p95_read_ms']} ms | {miss['summary']['mean_prefetch_wait_ms']} ms |

## Measured Inputs

- Hit saved latency: {data['measured_inputs']['hit_saved_ms']} ms
- Miss action cost: {data['measured_inputs']['miss_action_cost_ms']} ms
- Control-plane / dispatch cost: {data['measured_inputs']['control_plane_ms']} ms per action

## Same-Budget Baseline

Same-budget comparison inputs present: {has_baseline_inputs}.
Accepted: {accepted}.

This artifact is accepted for #97 only when n>=20 per mode, measured inputs are positive, same-budget hit-rate inputs are present for DiPECS and StrongPredictiveActionBaseline, DiPECS net benefit is positive, and DiPECS beats the strong baseline.
"""
with open(md_path, "w", encoding="utf-8") as f:
    f.write(md)

print(json_path)
print(md_path)
PY
}

if (( SAMPLES < 20 )); then
  echo "SAMPLES must be >=20 for #97 evidence; got $SAMPLES" >&2
  exit 1
fi

adb_cmd wait-for-device >/dev/null
adb_cmd forward --remove "tcp:$PORT" >/dev/null 2>&1 || true
adb_cmd forward "tcp:$PORT" "tcp:$PORT" >/dev/null
start_control

collect_prefetched_read
collect_miss_fetch_then_read
assemble_report
