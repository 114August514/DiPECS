#!/usr/bin/env bash
set -euo pipefail

# 脚本在 tests/scenarios/ 下,到仓库根是两层(../..)。
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export PATH="$PATH:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator"

TS="$(date +%Y%m%d-%H%M%S)"
mkdir -p logs data/evaluation data/traces
RUN_LOG="logs/action-loop-e2e-$TS.log"
PKG="com.dipecs.collector"
ACTION_PORT=46321
SAMPLE="data/traces/android_real_device_sample.redacted.jsonl"

source "$(dirname "${BASH_SOURCE[0]}")/lib/action-loop-stages.sh"
log "action-loop-e2e 启动 ts=$TS"

stage0_preflight
stage1_build_install
stage2_autostart_service
stage3_get_token
stage4_forward_port
stage5_send_action
stage6_verify_execution
write_validation_record
banner "完成 数据来源=$DATA_SOURCE"
