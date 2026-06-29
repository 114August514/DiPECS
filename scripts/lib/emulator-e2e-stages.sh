#!/usr/bin/env bash
# emulator-e2e 各阶段函数。被 scripts/emulator-e2e.sh source。

log()    { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*" | tee -a "$RUN_LOG"; }
die()    { printf '\n[FAIL] %s\n' "$*" | tee -a "$RUN_LOG" >&2; exit 1; }
banner() { printf '\n=== %s ===\n' "$*" | tee -a "$RUN_LOG"; }
