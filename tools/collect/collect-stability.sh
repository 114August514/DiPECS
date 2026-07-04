#!/usr/bin/env bash
set -euo pipefail
# Long-running memory stability test for DiPECS collector.
# Usage: DURATION_MINUTES=60 SAMPLE_INTERVAL_SECS=30 ./tools/collect/collect-stability.sh

ADB="${ADB:-}"
PACKAGE="${PACKAGE:-com.dipecs.collector}"
DURATION_MINUTES="${DURATION_MINUTES:-10}"
SAMPLE_INTERVAL_SECS="${SAMPLE_INTERVAL_SECS:-30}"
OUT_DIR="${OUT_DIR:-data/evaluation/stability}"

if [[ -z "$ADB" ]]; then
  if command -v adb >/dev/null 2>&1; then ADB="$(command -v adb)"
  elif [[ -x "/mnt/c/Users/33207/AppData/Local/Android/Sdk/platform-tools/adb.exe" ]]; then
    ADB="/mnt/c/Users/33207/AppData/Local/Android/Sdk/platform-tools/adb.exe"
  else echo "adb not found" >&2; exit 1; fi
fi

adb_cmd() { "$ADB" "$@"; }

get_meminfo() {
  local mem rss pss
  mem="$(adb_cmd shell dumpsys meminfo "$PACKAGE" 2>/dev/null || true)"
  rss="$(printf '%s\n' "$mem" | sed -n 's/.*TOTAL RSS:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  pss="$(printf '%s\n' "$mem" | sed -n 's/.*TOTAL PSS:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  echo "${rss:-0} ${pss:-0}"
}

get_top_cpu() {
  local line cpu
  line="$(adb_cmd shell top -b -n 1 -o PID,%CPU,RES,ARGS 2>/dev/null | grep "$PACKAGE" | head -n 1 || true)"
  if [[ -z "$line" ]]; then echo "0"; return; fi
  set -- $line; echo "$2"
}

adb_cmd wait-for-device >/dev/null
adb_cmd shell am force-stop "$PACKAGE" >/dev/null 2>&1 || true
sleep 2
adb_cmd shell am start -n "$PACKAGE/.debug.DebugCollectorControlActivity" >/dev/null 2>&1
sleep 4

total_samples=$(( DURATION_MINUTES * 60 / SAMPLE_INTERVAL_SECS ))
echo "Stability: ${DURATION_MINUTES}min, interval ${SAMPLE_INTERVAL_SECS}s ($total_samples samples)" >&2
mkdir -p "$OUT_DIR"
ts="$(date +%Y%m%d-%H%M%S)"
out="$OUT_DIR/stability-emulator-${ts}.jsonl"
: > "$out"

start_ts=$(date +%s)
for ((i=0; i<total_samples; i++)); do
  read -r rss_kb pss_kb < <(get_meminfo)
  cpu="$(get_top_cpu)"
  rss_mb=$(awk "BEGIN { printf \"%.3f\", $rss_kb / 1024 }")
  pss_mb=$(awk "BEGIN { printf \"%.3f\", $pss_kb / 1024 }")
  elapsed=$(( $(date +%s) - start_ts ))
  python3 - "$i" "$elapsed" "$cpu" "$rss_mb" "$pss_mb" <<'PY' >> "$out"
import json, sys
i, elapsed, cpu, rss, pss = sys.argv[1:]
obj = {"sample_index":int(i),"elapsed_secs":int(elapsed),"cpu_pct":float(cpu),"rss_mb":float(rss),"pss_mb":float(pss)}
print(json.dumps(obj, ensure_ascii=False))
PY
  echo "  [$i/$total_samples] +${elapsed}s  rss=${rss_mb}MB  pss=${pss_mb}MB  cpu=${cpu}%" >&2
  if [[ "$i" -lt $((total_samples - 1)) ]]; then sleep "$SAMPLE_INTERVAL_SECS"; fi
done

summary_json="$OUT_DIR/stability-emulator-${ts}.json"
adb_serial="$(adb_cmd get-serialno | tr -d '\r')"
python3 - "$out" "$summary_json" "$ts" "$DURATION_MINUTES" "$SAMPLE_INTERVAL_SECS" "$PACKAGE" "$adb_serial" <<'PY'
import json, sys
jsonl_path, json_path, ts, dur_min, interval, pkg, serial = sys.argv[1:]

samples = [json.loads(line) for line in open(jsonl_path, encoding="utf-8") if line.strip()]
n = len(samples)
rss_all = [s["rss_mb"] for s in samples]
pss_all = [s["pss_mb"] for s in samples]
cpu_all = [s["cpu_pct"] for s in samples]

def slope(xs, ys):
    mx = sum(xs) / len(xs)
    my = sum(ys) / len(ys)
    num = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    den = sum((x - mx) ** 2 for x in xs)
    return num / den if den != 0 else 0.0

# Skip first 2 samples (startup spike) for regression
if n > 4:
    reg = samples[2:]
    xs = list(range(len(reg)))
    rss_s = slope(xs, [s["rss_mb"] for s in reg])
    pss_s = slope(xs, [s["pss_mb"] for s in reg])
else:
    xs = list(range(n))
    rss_s = slope(xs, rss_all)
    pss_s = slope(xs, pss_all)

cpu_avg = sum(cpu_all) / n

rss_gph = rss_s * (3600 / int(interval))
pss_gph = pss_s * (3600 / int(interval))

dataset = {
    "schema_version": "dipecs.stability.v1",
    "dataset_id": f"stability-emulator-{ts}",
    "status": "measured_android_emulator",
    "environment": {
        "device": "Android Studio emulator", "package": pkg,
        "duration_minutes": int(dur_min), "sample_interval_secs": int(interval),
        "total_samples": n, "adb_serial": serial,
    },
    "results": {
        "rss_first_mb": rss_all[0], "rss_last_mb": rss_all[-1],
        "rss_delta_mb": round(rss_all[-1] - rss_all[0], 3),
        "rss_growth_per_hour_mb": round(rss_gph, 3),
        "pss_first_mb": pss_all[0], "pss_last_mb": pss_all[-1],
        "pss_delta_mb": round(pss_all[-1] - pss_all[0], 3),
        "pss_growth_per_hour_mb": round(pss_gph, 3),
        "avg_cpu_pct": round(cpu_avg, 3),
        "samples": samples,
    },
    "thresholds": {
        "max_rss_growth_per_hour_mb": 50.0, "max_pss_growth_per_hour_mb": 20.0,
        "max_avg_cpu_pct": 10.0,
    },
    "conclusion": {
        "accepted": rss_gph <= 50.0 and pss_gph <= 20.0 and cpu_avg <= 10.0,
        "note": "No significant memory leak detected" if rss_gph <= 50.0
                else f"WARNING: RSS growing at {rss_gph:.1f} MB/hour",
    },
}
with open(json_path, "w", encoding="utf-8") as f:
    json.dump(dataset, f, ensure_ascii=False, indent=2); f.write("\n")
print(f"Wrote {json_path}")
print(f"  RSS: {rss_all[0]:.1f} -> {rss_all[-1]:.1f} MB  growth={rss_gph:+.1f} MB/h")
print(f"  PSS: {pss_all[0]:.1f} -> {pss_all[-1]:.1f} MB  growth={pss_gph:+.1f} MB/h")
print(f"  CPU avg: {cpu_avg:.1f}%  Accepted: {dataset['conclusion']['accepted']}")
PY
echo "Wrote $summary_json"
