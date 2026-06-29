#!/usr/bin/env bash
set -euo pipefail

MODE="auto"
case "${1:-}" in
  --auto|"") MODE="auto" ;;
  --manual)  MODE="manual" ;;
  *) echo "用法: $0 [--auto|--manual]"; exit 2 ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export PATH="$PATH:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$ANDROID_HOME/cmdline-tools/latest/bin"

TS="$(date +%Y%m%d-%H%M%S)"
mkdir -p logs data/evaluation data/traces
RUN_LOG="logs/emulator-e2e-$TS.log"
AVD_NAME="dipecs_e2e"
PKG="com.dipecs.collector"

source "$REPO_ROOT/scripts/lib/emulator-e2e-stages.sh"
log "emulator-e2e 启动 mode=$MODE ts=$TS"

stage0_preflight
stage1_provision_sdk
stage2_boot_emulator
stage3_build_install
stage4_grant_and_start
stage5_generate_events
