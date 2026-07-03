#!/usr/bin/env bash
set -euo pipefail

# prepare-lsapp.sh — decompress + normalize the LSApp dataset for next-app eval.
#
# Data source (citation required):
#   LSApp — Large-scale mobile app usage dataset.
#   Aliannejadi, Zamani, Crestani, Croft. "Context-Aware Target Apps Selection
#   and Recommendation for Enhancing Personal Mobile Assistants." ACM TOIS 2021.
#   https://github.com/aliannejadi/LSApp
#   Upstream declares NO license; used here for research evaluation only.
#
# What this does:
#   third_party/LSApp/lsapp.tsv.gz is a gzipped TAR archive (single member
#   `lsapp.tsv`, ~179 MB). We stream that member with `tar -xzO` (NOT zcat —
#   zcat would emit the 512-byte ustar header as garbage) and rewrite the
#   `timestamp` column into an integer epoch-ms column named `timestamp_ms`.
#   The downstream loader (crates/aios-cli/src/next_app.rs) parses the
#   `timestamp_ms` column as integer ms directly; a datetime string there would
#   silently fall back to ordinal*1000 and destroy event ordering.
#
# Timezone: the raw `YYYY-MM-DD HH:MM:SS` strings carry no zone. We parse them
#   as UTC (calendar.timegm) for determinism — hour buckets then equal the
#   literal clock hour in the data and the output never depends on the runner's
#   local timezone.
#
# Streaming + constant memory: one pass, piped straight to a temp file, then an
#   atomic mv into place. The 179 MB member is never held in memory.
#
# Usage:
#   bash tools/prepare-lsapp.sh          # run from the repo/worktree root
#   FORCE=1 bash tools/prepare-lsapp.sh  # regenerate even if output exists

PYTHON="${PYTHON:-}"
SRC_GZ="${SRC_GZ:-third_party/LSApp/lsapp.tsv.gz}"
MEMBER="${MEMBER:-lsapp.tsv}"
OUT_DIR="${OUT_DIR:-data/lsapp}"
OUT_FILE="${OUT_FILE:-$OUT_DIR/lsapp.tsv}"
FORCE="${FORCE:-0}"

if [[ -z "$PYTHON" ]]; then
  if command -v python3 >/dev/null 2>&1; then
    PYTHON="$(command -v python3)"
  else
    echo "python3 not found. Set PYTHON=/path/to/python3" >&2
    exit 1
  fi
fi

# Preflight: the submodule blob must be present.
if [[ ! -f "$SRC_GZ" ]]; then
  echo "error: $SRC_GZ not found." >&2
  echo "The LSApp submodule is not initialized. Run:" >&2
  echo "  git submodule update --init third_party/LSApp" >&2
  exit 1
fi

# Idempotent: skip unless FORCE=1.
if [[ -f "$OUT_FILE" && "$FORCE" != "1" ]]; then
  echo "prepare-lsapp: $OUT_FILE already exists; skipping (set FORCE=1 to regenerate)." >&2
  exit 0
fi

read -r -d '' CONVERTER <<'PY' || true
import sys, time, calendar

# Locale-independent I/O; surrogateescape round-trips any odd bytes in app names.
sys.stdin.reconfigure(encoding="utf-8", errors="surrogateescape")
sys.stdout.reconfigure(encoding="utf-8", errors="surrogateescape")

stdin = sys.stdin
write = sys.stdout.write

header = stdin.readline()
if not header:
    sys.stderr.write("prepare-lsapp: empty input\n")
    sys.exit(1)

cols = header.rstrip("\r\n").split("\t")

# Locate the timestamp column; rename it to timestamp_ms in place. If the input
# is already named timestamp_ms, keep it (no second header prepended either way).
ts_idx = None
for i, c in enumerate(cols):
    if c.strip().lower() == "timestamp_ms":
        ts_idx = i
        break
if ts_idx is None:
    for i, c in enumerate(cols):
        if c.strip().lower() == "timestamp":
            ts_idx = i
            cols[i] = "timestamp_ms"
            break
if ts_idx is None:
    sys.stderr.write("prepare-lsapp: no 'timestamp' column in header: %r\n" % (cols,))
    sys.exit(1)

ncols = len(cols)
write("\t".join(cols) + "\n")

cache = {}
skipped = 0
for line in stdin:
    line = line.rstrip("\r\n")
    if not line:
        continue
    fields = line.split("\t")
    if len(fields) != ncols:
        skipped += 1
        continue
    v = fields[ts_idx]
    ms = cache.get(v)
    if ms is None:
        try:
            ms = calendar.timegm(time.strptime(v, "%Y-%m-%d %H:%M:%S")) * 1000
        except (ValueError, OverflowError):
            skipped += 1
            continue
        cache[v] = ms
    fields[ts_idx] = str(ms)
    write("\t".join(fields) + "\n")

sys.stderr.write("prepare-lsapp: skipped %d malformed/unparseable line(s)\n" % skipped)
PY

mkdir -p "$OUT_DIR"
tmp="$(mktemp "$OUT_DIR/.lsapp.tsv.XXXXXX")"
trap 'rm -f "$tmp"' EXIT

tar -xzO -f "$SRC_GZ" "$MEMBER" | "$PYTHON" -c "$CONVERTER" > "$tmp"
mv "$tmp" "$OUT_FILE"
trap - EXIT

echo "prepare-lsapp: wrote $OUT_FILE" >&2
