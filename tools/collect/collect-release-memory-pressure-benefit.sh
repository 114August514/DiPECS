#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
ADB="${ADB:-adb}"
PACKAGE="${PACKAGE:-com.dipecs.collector}"
SAMPLES="${SAMPLES:-20}"
SAMPLE_INTERVAL_SECS="${SAMPLE_INTERVAL_SECS:-1}"
PRESSURE_WINDOW_SECS="${PRESSURE_WINDOW_SECS:-12}"
POST_ACTION_WINDOW_SECS="${POST_ACTION_WINDOW_SECS:-4}"
# PRESSURE_COMMAND must HOLD memory pressure for the full measurement window
# (roughly PRESSURE_WINDOW_SECS + POST_ACTION_WINDOW_SECS). If it releases its
# memory on exit before mem_before/mem_after are sampled, both arms see ~0 gain,
# the Welch t-test comes back non-significant, and the gate fails closed (no
# false accept) — but you also get no usable evidence. Use a command that blocks
# and keeps its allocation resident across the whole window.
PRESSURE_COMMAND="${PRESSURE_COMMAND:-}"
TOKEN="${TOKEN:-dipecs-dev-emulator-shared-token-00000000}"
PORT="${PORT:-46321}"
ACTION_HOST="${ACTION_HOST:-127.0.0.1}"
DELAY="${DELAY:-1.0}"
RELEASE_TARGET="${RELEASE_TARGET:-cache:prefetch}"
SEED_VOLATILE_CACHE_MB="${SEED_VOLATILE_CACHE_MB:-0}"
OUT_DIR="${OUT_DIR:-$REPO_ROOT/data/evaluation/action-net-benefit}"
SENDER="$REPO_ROOT/tests/scenarios/lib/action-forensic-sender.py"

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

send_release_memory() {
  local line latency_us
  line="$(python3 "$SENDER" "$ACTION_HOST" "$PORT" "$TOKEN" "$DELAY" ReleaseMemory "$RELEASE_TARGET" Immediate 2>&1)"
  latency_us="$(LINE="$line" python3 - <<'PY'
import json
import os
import re

text = os.environ["LINE"]
m = re.search(r"device=({.*?})", text)
if not m:
    raise SystemExit("ReleaseMemory missing device response")
try:
    data = json.loads(m.group(1))
except Exception as err:
    raise SystemExit(f"ReleaseMemory invalid device response: {err}")
if data.get("status") != "ok":
    raise SystemExit(f"ReleaseMemory bridge did not accept action: {data}")
latency_us = int(data.get("latency_us") or 0)
if latency_us <= 0:
    raise SystemExit("ReleaseMemory missing positive latency_us")
print(latency_us)
PY
)"
  printf '%s\t%s\n' "$latency_us" "$line"
}

seed_volatile_cache() {
  if (( SEED_VOLATILE_CACHE_MB <= 0 )); then
    return
  fi
  local target line status summary
  target="own:volatile-cache:$SEED_VOLATILE_CACHE_MB"
  line="$(python3 "$SENDER" "$ACTION_HOST" "$PORT" "$TOKEN" "$DELAY" PreWarmProcess "$target" Immediate 2>&1)"
  read -r status summary < <(LINE="$line" python3 - <<'PY'
import json
import os
import re

text = os.environ["LINE"]
m = re.search(r"device=({.*?})", text)
if not m:
    raise SystemExit("PreWarmProcess volatile cache seed missing device response")
data = json.loads(m.group(1))
print(data.get("status", ""), data.get("summary", ""))
PY
)
  if [[ "$status" != "ok" ]]; then
    echo "Volatile cache seed failed: $line" >&2
    return 1
  fi
  echo "seed_volatile_cache target=$target summary=$summary" >&2
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

start_pressure_window() {
  if [[ -z "$PRESSURE_COMMAND" ]]; then
    echo "PRESSURE_COMMAND is required for #99 evidence" >&2
    exit 1
  fi
  export ADB PACKAGE PRESSURE_WINDOW_SECS
  bash -lc "$PRESSURE_COMMAND" &
  PRESSURE_PID=$!
  sleep "$PRESSURE_WINDOW_SECS"
  if ! kill -0 "$PRESSURE_PID" >/dev/null 2>&1; then
    wait "$PRESSURE_PID" || true
    echo "PRESSURE_COMMAND exited before the post-pressure measurement window" >&2
    return 1
  fi
}

finish_pressure_window() {
  local pressure_pid="$1"
  if [[ -n "$pressure_pid" ]]; then
    wait "$pressure_pid" || {
      echo "PRESSURE_COMMAND failed" >&2
      return 1
    }
  fi
}

write_sample() {
  local file="$1"
  local idx="$2"
  local mode="$3"
  local release_latency_us="$4"
  local pid_before="$5"
  local pid_after="$6"
  local mem_pressure_start="$7"
  local mem_before_action="$8"
  local mem_after_action="$9"
  local pss_before_action="${10}"
  local pss_after_action="${11}"
  local jank_before="${12}"
  local jank_after="${13}"
  local ts mem_pressure_drop mem_recovered pss_reduction jank_delta
  ts="$(date -u +%s%3N)"
  mem_pressure_drop="$(python3 - "$mem_pressure_start" "$mem_before_action" <<'PY'
import sys
print(int(float(sys.argv[1])) - int(float(sys.argv[2])))
PY
)"
  mem_recovered="$(python3 - "$mem_before_action" "$mem_after_action" <<'PY'
import sys
print(int(float(sys.argv[2])) - int(float(sys.argv[1])))
PY
)"
  pss_reduction="$(python3 - "$pss_before_action" "$pss_after_action" <<'PY'
import sys
print(int(float(sys.argv[1])) - int(float(sys.argv[2])))
PY
)"
  jank_delta="$(python3 - "$jank_before" "$jank_after" <<'PY'
import sys
print(f"{float(sys.argv[2]) - float(sys.argv[1]):.3f}")
PY
)"
  printf '{"sample_index":%d,"timestamp_ms":%d,"mode":"%s","release_latency_us":%d,"pid_before":"%s","pid_after":"%s","mem_available_pressure_start_kb":%d,"mem_available_before_action_kb":%d,"mem_available_after_action_kb":%d,"mem_available_pressure_drop_kb":%d,"mem_available_recovered_kb":%d,"pss_before_action_kb":%d,"pss_after_action_kb":%d,"pss_reduction_kb":%d,"jank_before_pct":%.3f,"jank_after_pct":%.3f,"jank_delta_pct_points":%.3f}\n' \
    "$idx" "$ts" "$mode" "$release_latency_us" "$pid_before" "$pid_after" \
    "$mem_pressure_start" "$mem_before_action" "$mem_after_action" \
    "$mem_pressure_drop" "$mem_recovered" "$pss_before_action" "$pss_after_action" \
    "$pss_reduction" "$jank_before" "$jank_after" "$jank_delta" >> "$file"
}

collect_mode() {
  local mode="$1"
  local with_release="$2"
  local file="$raw_dir/$mode.jsonl"
  : > "$file"
  for ((i=0; i<SAMPLES; i++)); do
    adb_cmd shell am force-stop "$PACKAGE" >/dev/null 2>&1 || true
    start_control
    seed_volatile_cache
    reset_gfxinfo
    local pid_before mem_start mem_before pss_before jank_before release_latency
    local pid_after mem_after pss_after jank_after pressure_pid
    pid_before="$(pidof_package)"
    mem_start="$(mem_available_kb)"
    start_pressure_window
    pressure_pid="$PRESSURE_PID"
    mem_before="$(mem_available_kb)"
    pss_before="$(pss_kb "$pid_before")"
    jank_before="$(jank_pct)"
    release_latency=0
    if [[ "$with_release" == "true" ]]; then
      read -r release_latency _ < <(send_release_memory)
      sleep "$POST_ACTION_WINDOW_SECS"
    else
      sleep "$POST_ACTION_WINDOW_SECS"
    fi
    pid_after="$(pidof_package)"
    mem_after="$(mem_available_kb)"
    pss_after="$(pss_kb "$pid_after")"
    jank_after="$(jank_pct)"
    finish_pressure_window "$pressure_pid"
    write_sample "$file" "$i" "$mode" "$release_latency" "$pid_before" "$pid_after" \
      "$mem_start" "$mem_before" "$mem_after" "$pss_before" "$pss_after" \
      "$jank_before" "$jank_after"
    echo "$mode[$i] mem_before=${mem_before}KB mem_after=${mem_after}KB pss_before=${pss_before}KB pss_after=${pss_after}KB jank_before=${jank_before}% jank_after=${jank_after}%" >&2
    sleep "$SAMPLE_INTERVAL_SECS"
  done
}

assemble_report() {
  local json_path="$OUT_DIR/release-memory-pressure-benefit-$timestamp.json"
  local md_path="$OUT_DIR/release-memory-pressure-benefit-$timestamp.md"
  local serial
  serial="$(adb_cmd get-serialno | tr -d '\r')"
  python3 - \
    "$raw_dir/baseline_pressure.jsonl" \
    "$raw_dir/release_memory_pressure.jsonl" \
    "$json_path" "$md_path" "$timestamp" "$serial" "$PACKAGE" "$RELEASE_TARGET" \
    "$SEED_VOLATILE_CACHE_MB" "$REPO_ROOT" <<'PY'
import datetime
import json
import math
import pathlib
import statistics
import sys


def welch_t_test(a, b):
    """Welch's t-test (two-sided) for unequal-variance independent samples.

    Returns (t_statistic, p_value, degrees_of_freedom).
    """
    n_a, n_b = len(a), len(b)
    if n_a < 2 or n_b < 2:
        return (0.0, 1.0, 0.0)
    mean_a, mean_b = statistics.mean(a), statistics.mean(b)
    var_a = statistics.variance(a)
    var_b = statistics.variance(b)
    se_a = var_a / n_a
    se_b = var_b / n_b
    se = math.sqrt(se_a + se_b)
    if se == 0:
        if mean_a != mean_b:
            return (
                math.inf if mean_a > mean_b else -math.inf,
                0.0,
                min(n_a, n_b) - 1,
            )
        return (0.0, 1.0, min(n_a, n_b) - 1)
    t_stat = (mean_a - mean_b) / se
    # Welch–Satterthwaite degrees of freedom
    numerator = (se_a + se_b) ** 2
    denominator = (se_a ** 2 / (n_a - 1)) + (se_b ** 2 / (n_b - 1))
    df = numerator / denominator if denominator > 0 else 1.0
    # Two-sided p-value via regularised incomplete beta function (no scipy)
    x = df / (df + t_stat ** 2)
    p_value = _regularised_incomplete_beta(df / 2.0, 0.5, x)
    return (round(t_stat, 6), round(p_value, 8), round(df, 2))


def _regularised_incomplete_beta(a, b, x):
    """I_x(a, b) via continued fraction (Lentz's method).  0 <= x <= 1."""
    if x <= 0:
        return 0.0
    if x >= 1:
        return 1.0
    # Use symmetry relation if x > (a+1)/(a+b+2) for faster convergence
    if x > (a + 1) / (a + b + 2):
        return 1.0 - _regularised_incomplete_beta(b, a, 1.0 - x)
    front = math.exp(
        math.lgamma(a + b) - math.lgamma(a) - math.lgamma(b)
        + a * math.log(x) + b * math.log(1.0 - x)
    ) / a
    # Continued fraction
    cf = _continued_fraction_beta(a, b, x)
    return front * cf


def _continued_fraction_beta(a, b, x, max_iter=200, tol=1e-14):
    """Evaluate continued fraction for I_x(a, b) using Lentz's algorithm."""
    qab = a + b
    qap = a + 1.0
    qam = a - 1.0
    c = 1.0
    d = 1.0 - qab * x / qap
    if abs(d) < 1e-30:
        d = 1e-30
    d = 1.0 / d
    h = d
    for m in range(1, max_iter + 1):
        m2 = 2 * m
        # Even step
        aa = m * (b - m) * x / ((qam + m2) * (a + m2))
        d = 1.0 + aa * d
        if abs(d) < 1e-30:
            d = 1e-30
        c = 1.0 + aa / c
        if abs(c) < 1e-30:
            c = 1e-30
        d = 1.0 / d
        h *= d * c
        # Odd step
        aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2))
        d = 1.0 + aa * d
        if abs(d) < 1e-30:
            d = 1e-30
        c = 1.0 + aa / c
        if abs(c) < 1e-30:
            c = 1e-30
        d = 1.0 / d
        delta = d * c
        h *= delta
        if abs(delta - 1.0) < tol:
            break
    return h

(
    baseline_p,
    release_p,
    json_path,
    md_path,
    timestamp,
    serial,
    package,
    release_target,
    seed_volatile_cache_mb,
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
    mem_drop = [float(s["mem_available_pressure_drop_kb"]) for s in samples]
    mem_recovered = [float(s["mem_available_recovered_kb"]) for s in samples]
    pss_reduction = [float(s["pss_reduction_kb"]) for s in samples]
    jank_delta = [float(s["jank_delta_pct_points"]) for s in samples]
    latencies = [float(s.get("release_latency_us") or 0) for s in samples]
    return {
        "mode": mode,
        "samples": samples,
        "summary": {
            "n": len(samples),
            "mean_mem_available_pressure_drop_kb": round(statistics.mean(mem_drop), 3),
            "mean_mem_available_recovered_kb": round(statistics.mean(mem_recovered), 3),
            "p95_mem_available_recovered_kb": round(percentile(mem_recovered, 95.0), 3),
            "mean_pss_reduction_kb": round(statistics.mean(pss_reduction), 3),
            "p95_pss_reduction_kb": round(percentile(pss_reduction, 95.0), 3),
            "mean_jank_delta_pct_points": round(statistics.mean(jank_delta), 3),
            "p95_jank_delta_pct_points": round(percentile(jank_delta, 95.0), 3),
            "mean_release_latency_us": round(statistics.mean(latencies), 3),
        },
    }

baseline = load(baseline_p, "baseline_pressure")
release = load(release_p, "release_memory_pressure")

# Extract per-sample raw arrays for statistical testing
baseline_raw = baseline["samples"]
release_raw = release["samples"]
baseline_mem_recovered = [float(s["mem_available_recovered_kb"]) for s in baseline_raw]
release_mem_recovered = [float(s["mem_available_recovered_kb"]) for s in release_raw]
baseline_pss_reduction = [float(s["pss_reduction_kb"]) for s in baseline_raw]
release_pss_reduction = [float(s["pss_reduction_kb"]) for s in release_raw]
baseline_jank_delta = [float(s["jank_delta_pct_points"]) for s in baseline_raw]
release_jank_delta = [float(s["jank_delta_pct_points"]) for s in release_raw]

available_gain_kb = (
    release["summary"]["mean_mem_available_recovered_kb"]
    - baseline["summary"]["mean_mem_available_recovered_kb"]
)
pss_reduction_gain_kb = (
    release["summary"]["mean_pss_reduction_kb"]
    - baseline["summary"]["mean_pss_reduction_kb"]
)
jank_delta_vs_baseline_pct_points = (
    release["summary"]["mean_jank_delta_pct_points"]
    - baseline["summary"]["mean_jank_delta_pct_points"]
)
control_plane_ms = release["summary"]["mean_release_latency_us"] / 1000.0

# Welch's t-tests (two-sided) for each key metric
SIGNIFICANCE_ALPHA = 0.05
t_available, p_available, df_available = welch_t_test(
    release_mem_recovered, baseline_mem_recovered
)
t_pss, p_pss, df_pss = welch_t_test(
    release_pss_reduction, baseline_pss_reduction
)
t_jank, p_jank, df_jank = welch_t_test(
    release_jank_delta, baseline_jank_delta
)
# Directional checks: available memory should increase. PSS and jank are safety
# gates: they should not regress, but they do not need to improve significantly.
directional_available_ok = available_gain_kb > 0
directional_pss_ok = pss_reduction_gain_kb >= 0
directional_jank_ok = jank_delta_vs_baseline_pct_points <= 0
available_memory_significant = p_available < SIGNIFICANCE_ALPHA
statistically_significant = available_memory_significant

n_at_least_20_per_mode = all(run["summary"]["n"] >= 20 for run in [baseline, release])
# AND across both arms (not OR): a valid comparison requires that BOTH the
# baseline and release runs actually experienced memory pressure. OR would let a
# run where only one arm was stressed pass, making the available-memory delta an
# apples-to-oranges artifact rather than a like-for-like pressure comparison.
memory_pressure_observed = (
    baseline["summary"]["mean_mem_available_pressure_drop_kb"] > 0
    and release["summary"]["mean_mem_available_pressure_drop_kb"] > 0
)
measured_inputs_valid = (
    math.isfinite(available_gain_kb)
    and math.isfinite(pss_reduction_gain_kb)
    and math.isfinite(jank_delta_vs_baseline_pct_points)
    and math.isfinite(control_plane_ms)
    and control_plane_ms > 0
)
release_memory_effective = (
    directional_available_ok
    and directional_pss_ok
    and directional_jank_ok
)
accepted = (
    n_at_least_20_per_mode
    and memory_pressure_observed
    and measured_inputs_valid
    and release_memory_effective
    and statistically_significant
)
if accepted:
    status = "measured_android_device"
elif n_at_least_20_per_mode and memory_pressure_observed and measured_inputs_valid:
    status = "measured_no_significant_benefit"
else:
    status = "measurement_pending_pressure_gate"

data = {
    "schema_version": "dipecs.release_memory_pressure_benefit.v1",
    "dataset_id": f"release-memory-pressure-benefit-{timestamp}",
    "action": "ReleaseMemory",
    "source": "measured_device",
    "status": status,
    "environment": {
        "device": "Android adb target",
        "adb_serial": serial,
        "package": package,
        "release_target": release_target,
        "seed_volatile_cache_mb": int(seed_volatile_cache_mb),
        "samples_per_mode": baseline["summary"]["n"],
        "collected_at": datetime.datetime.now().isoformat(timespec="seconds"),
    },
    "provenance": {
        "measurement": "Compare memory/jank deltas under an explicit memory pressure command with and without ReleaseMemory.",
        "pressure_command_required": True,
        "raw_baseline_samples": rel(baseline_p),
        "raw_release_samples": rel(release_p),
    },
    "runs": [baseline, release],
    "measured_inputs": {
        "source": "measured_device",
        "available_gain_kb": round(available_gain_kb, 3),
        "pss_reduction_gain_kb": round(pss_reduction_gain_kb, 3),
        "jank_delta_vs_baseline_pct_points": round(jank_delta_vs_baseline_pct_points, 3),
        "control_plane_ms": round(control_plane_ms, 3),
    },
    "statistical_tests": {
        "method": "Welch's t-test (two-sided)",
        "significance_alpha": SIGNIFICANCE_ALPHA,
        "available_memory": {
            "t_statistic": t_available,
            "p_value": p_available,
            "df": df_available,
            "required_for_acceptance": True,
        },
        "pss_reduction": {
            "t_statistic": t_pss,
            "p_value": p_pss,
            "df": df_pss,
            "required_for_acceptance": False,
        },
        "jank_delta": {
            "t_statistic": t_jank,
            "p_value": p_jank,
            "df": df_jank,
            "required_for_acceptance": False,
        },
        "available_memory_significant": available_memory_significant,
        "pss_non_regression_required": directional_pss_ok,
        "jank_non_regression_required": directional_jank_ok,
        "statistically_significant": statistically_significant,
    },
    "conclusion": {
        "accepted": accepted,
        "n_at_least_20_per_mode": n_at_least_20_per_mode,
        "memory_pressure_observed": memory_pressure_observed,
        "measured_inputs_valid": measured_inputs_valid,
        "release_memory_effective": release_memory_effective,
        "statistically_significant": statistically_significant,
    },
}

with open(json_path, "w", encoding="utf-8") as f:
    json.dump(data, f, ensure_ascii=False, indent=2)
    f.write("\n")

md = f"""# DiPECS ReleaseMemory Pressure Benefit Measurement

- Dataset: `{pathlib.Path(json_path).name}`
- Status: {data['status']}
- Release target: `{release_target}`
- Samples per mode: {baseline['summary']['n']}

## Memory Pressure

| Mode | Mean pressure drop | Mean available recovered | p95 recovered | Mean PSS reduction | Mean jank delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| baseline pressure | {baseline['summary']['mean_mem_available_pressure_drop_kb']} KB | {baseline['summary']['mean_mem_available_recovered_kb']} KB | {baseline['summary']['p95_mem_available_recovered_kb']} KB | {baseline['summary']['mean_pss_reduction_kb']} KB | {baseline['summary']['mean_jank_delta_pct_points']} pp |
| release memory pressure | {release['summary']['mean_mem_available_pressure_drop_kb']} KB | {release['summary']['mean_mem_available_recovered_kb']} KB | {release['summary']['p95_mem_available_recovered_kb']} KB | {release['summary']['mean_pss_reduction_kb']} KB | {release['summary']['mean_jank_delta_pct_points']} pp |

## Measured Inputs

- Available-memory gain over baseline: {data['measured_inputs']['available_gain_kb']} KB
- PSS reduction gain over baseline: {data['measured_inputs']['pss_reduction_gain_kb']} KB
- Jank delta vs baseline: {data['measured_inputs']['jank_delta_vs_baseline_pct_points']} pp
- Control-plane / dispatch cost: {data['measured_inputs']['control_plane_ms']} ms per action

## Statistical Significance (Welch's t-test, alpha={SIGNIFICANCE_ALPHA})

| Metric | t-statistic | p-value | df | Required for acceptance |
| --- | ---: | ---: | ---: | --- |
| Available memory gain | {t_available} | {p_available} | {df_available} | Significant positive gain |
| PSS reduction gain | {t_pss} | {p_pss} | {df_pss} | Non-regression only |
| Jank delta | {t_jank} | {p_jank} | {df_jank} | Non-regression only |

Available-memory gain statistically significant: **{available_memory_significant}**.
PSS non-regression: **{directional_pss_ok}**.
Jank non-regression: **{directional_jank_ok}**.

## Acceptance

Accepted: {accepted}.

This artifact is accepted for #99 only when n>=20 per mode, memory pressure is observed, measurements are valid, available memory improves over baseline with Welch's t-test p < {SIGNIFICANCE_ALPHA}, PSS reduction is not worse, and jank does not regress.
"""
with open(md_path, "w", encoding="utf-8") as f:
    f.write(md)

print(json_path)
print(md_path)
PY
}

if (( SAMPLES < 20 )); then
  echo "SAMPLES must be >=20 for #99 evidence; got $SAMPLES" >&2
  exit 1
fi
if [[ -z "$PRESSURE_COMMAND" ]]; then
  echo "PRESSURE_COMMAND is required for #99 evidence" >&2
  exit 1
fi

adb_cmd wait-for-device >/dev/null
adb_cmd forward --remove "tcp:$PORT" >/dev/null 2>&1 || true
adb_cmd forward "tcp:$PORT" "tcp:$PORT" >/dev/null

# The bridge validates envelope freshness against the device wall clock
# (AuthorizedActionSocketServer.hasFreshActionWindow, ±30s skew). A device whose
# system clock is not synced (real device without network/NITZ) rejects
# host-clock envelopes as expired. Measure device-minus-host offset once and
# export it so every action-forensic-sender.py dispatch aligns to the device
# clock. Emulators / correctly-synced devices measure ~0 and behave as before.
if [[ -z "${DEVICE_CLOCK_OFFSET_MS:-}" ]]; then
  _dev_ms="$(adb_cmd shell date +%s%3N | tr -d '\r')"
  _host_ms="$(date +%s%3N)"
  DEVICE_CLOCK_OFFSET_MS=$(( _dev_ms - _host_ms ))
fi
export DEVICE_CLOCK_OFFSET_MS
echo "device clock offset = ${DEVICE_CLOCK_OFFSET_MS} ms" >&2

collect_mode baseline_pressure false
collect_mode release_memory_pressure true
assemble_report
