#!/bin/bash
# scripts/android/run-daemon-in-emulator.sh
#
# Cross-compile dipecsd for the Android emulator (x86_64), push it, and
# launch it in foreground mode with the collector app's JSONL path.
#
# Usage:
#   ./scripts/android/run-daemon-in-emulator.sh [--attach]
#
# Prerequisites:
#   - NDK r27d installed, ANDROID_NDK_HOME set
#   - Emulator running with adb-accessible device
#   - DiPECS Android app installed and collector service started

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ── Configuration ─────────────────────────────────────────────────

TARGET="x86_64-linux-android"
ANDROID_API="${ANDROID_API:-33}"
EMULATOR_JSONL_PATH="${EMULATOR_JSONL_PATH:-/data/data/com.dipecs.collector/files/actions.jsonl}"
EMULATOR_BIN_DIR="${EMULATOR_BIN_DIR:-/data/local/tmp/dipecs}"
EMULATOR_TRACE_OUTPUT="${EMULATOR_TRACE_OUTPUT:-/data/local/tmp/dipecs/runtime_trace.ndjson}"
BRIDGE_HOST="${BRIDGE_HOST:-127.0.0.1}"
BRIDGE_PORT="${BRIDGE_PORT:-46321}"
BRIDGE_TOKEN="${BRIDGE_TOKEN:-dipecs-dev-emulator-shared-token-00000000}"
ATTACH_MODE=false

for arg in "$@"; do
    case "$arg" in
        --attach) ATTACH_MODE=true ;;
    esac
done

# ── Colour ────────────────────────────────────────────────────────

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${YELLOW}=== dipecsd emulator runner ===${NC}"

# ── 1. Detect toolchain ──────────────────────────────────────────

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    DEFAULT_NDK="$HOME/Android/ndk/android-ndk-r27d"
    if [ -d "$DEFAULT_NDK" ]; then
        export ANDROID_NDK_HOME="$DEFAULT_NDK"
    else
        echo -e "${RED}ANDROID_NDK_HOME not set and default NDK not found.${NC}"
        echo "Set ANDROID_NDK_HOME or install NDK r27d to $DEFAULT_NDK"
        exit 1
    fi
fi

OS_TYPE=$(uname -s | tr '[:upper:]' '[:lower:]')
TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/${OS_TYPE}-x86_64"
LINKER_NAME="${TARGET}${ANDROID_API}-clang"

if [ ! -x "$TOOLCHAIN/bin/$LINKER_NAME" ]; then
    echo -e "${RED}Linker $LINKER_NAME not found in $TOOLCHAIN/bin${NC}"
    exit 1
fi

export PATH="$TOOLCHAIN/bin:$PATH"
export "CARGO_TARGET_$(echo "$TARGET" | tr '-' '_' | tr '[:lower:]' '[:upper:]')_LINKER=$LINKER_NAME"

echo -e "  TARGET:    ${GREEN}$TARGET${NC}"
echo -e "  LINKER:    ${GREEN}$LINKER_NAME${NC}"

# ── 2. Check adb connectivity ────────────────────────────────────

ADB="adb"
if ! $ADB get-state &>/dev/null; then
    echo -e "${RED}No adb device found. Is the emulator running?${NC}"
    exit 1
fi

EMULATOR_SERIAL=$($ADB devices | grep -o 'emulator-[0-9]*' | head -1 || true)
if [ -z "$EMULATOR_SERIAL" ]; then
    echo -e "${YELLOW}No emulator serial found; using default adb target.${NC}"
else
    ADB="adb -s $EMULATOR_SERIAL"
    echo -e "  DEVICE:    ${GREEN}$EMULATOR_SERIAL${NC}"
fi

# ── 3. Cross-compile ─────────────────────────────────────────────

echo -e "\n${YELLOW}Cross-compiling dipecsd for $TARGET...${NC}"
cd "$REPO_ROOT"
cargo build -p aios-daemon --target "$TARGET" --release 2>&1 | tail -3

BINARY="$REPO_ROOT/target/$TARGET/release/dipecsd"
if [ ! -f "$BINARY" ]; then
    echo -e "${RED}Build failed: binary not found at $BINARY${NC}"
    exit 1
fi
echo -e "  BINARY:    ${GREEN}$BINARY${NC}"

# ── 4. Push to emulator ──────────────────────────────────────────

echo -e "\n${YELLOW}Pushing dipecsd to emulator...${NC}"
$ADB shell "mkdir -p $EMULATOR_BIN_DIR" 2>/dev/null || true
$ADB push "$BINARY" "$EMULATOR_BIN_DIR/dipecsd" | tail -1
$ADB shell "chmod +x $EMULATOR_BIN_DIR/dipecsd"

echo -e "  Pushed to: ${GREEN}$EMULATOR_BIN_DIR/dipecsd${NC}"

# ── 5. Assemble env and args ─────────────────────────────────────

DAEMON_ENV=(
    "DIPECS_ANDROID_TRACE_JSONL=$EMULATOR_JSONL_PATH"
    "DIPECS_RUNTIME_TRACE_OUTPUT=$EMULATOR_TRACE_OUTPUT"
    "DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=true"
    "DIPECS_ANDROID_ACTION_BRIDGE_HOST=$BRIDGE_HOST"
    "DIPECS_ANDROID_ACTION_BRIDGE_PORT=$BRIDGE_PORT"
    "DIPECS_ANDROID_ACTION_BRIDGE_TOKEN=$BRIDGE_TOKEN"
    "DIPECS_CLOUD_LLM_ENABLED=false"
    "RUST_LOG=dipecs=debug"
)

ENV_PREFIX=""
for var in "${DAEMON_ENV[@]}"; do
    ENV_PREFIX="$ENV_PREFIX$var "
done

# Truncate previous trace file.
$ADB shell "rm -f $EMULATOR_TRACE_OUTPUT" 2>/dev/null || true

echo -e "\n${YELLOW}Launching dipecsd in emulator...${NC}"
echo -e "  JSONL:     ${GREEN}$EMULATOR_JSONL_PATH${NC}"
echo -e "  TRACE:     ${GREEN}$EMULATOR_TRACE_OUTPUT${NC}"
echo -e "  BRIDGE:    ${GREEN}$BRIDGE_HOST:$BRIDGE_PORT${NC}"

# ── 6. Run ────────────────────────────────────────────────────────

run_daemon() {
    $ADB shell "env $ENV_PREFIX $EMULATOR_BIN_DIR/dipecsd --no-daemon"
}

if $ATTACH_MODE; then
    echo -e "\n${YELLOW}Running in foreground (Ctrl-C to stop)...${NC}"
    echo "------------------------------------------"
    run_daemon
    EXIT_CODE=$?
    echo "------------------------------------------"
    if [ $EXIT_CODE -ne 0 ]; then
        echo -e "${RED}dipecsd exited with code $EXIT_CODE${NC}"
    else
        echo -e "${GREEN}dipecsd stopped cleanly.${NC}"
    fi
else
    # Run in background on the emulator.
    echo -e "\n${YELLOW}Starting dipecsd in background on emulator...${NC}"
    $ADB shell "env $ENV_PREFIX nohup $EMULATOR_BIN_DIR/dipecsd --no-daemon > /data/local/tmp/dipecsd/stdout.log 2> /data/local/tmp/dipecsd/stderr.log &"
    DAEMON_PID=$($ADB shell "pidof dipecsd" 2>/dev/null | tr -d '\r' || true)
    if [ -z "$DAEMON_PID" ]; then
        echo -e "${YELLOW}Daemon may take a moment to start. Check logs:${NC}"
        echo "  adb shell cat /data/local/tmp/dipecsd/stdout.log"
    else
        echo -e "  PID:       ${GREEN}$DAEMON_PID${NC}"
    fi
    echo ""
    echo -e "${GREEN}dipecsd is running on the emulator.${NC}"
    echo ""
    echo "Monitor:"
    echo "  adb shell tail -f /data/local/tmp/dipecsd/stdout.log"
    echo "  adb shell cat $EMULATOR_TRACE_OUTPUT"
    echo ""
    echo "Stop:"
    echo "  adb shell kill \$(adb shell pidof dipecsd)"
    echo ""
    echo "Fetch trace:"
    echo "  adb pull $EMULATOR_TRACE_OUTPUT ./runtime_trace.ndjson"
    echo ""
fi
