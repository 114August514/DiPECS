#!/usr/bin/env bash
# ───────────────────────────────────────────────────────────────
# ship-system.sh — Deploy DiPECS as a system-privileged app
#
# Prerequisites for a full system deploy (AOSP or rooted device):
#   1. Android platform signing keys (platform.pk8 + platform.x509.pem)
#   2. ADB with root access (`adb root`)
#   3. Remounted /system partition (`adb remount`)
#
# For development / custom ROMs:
#   - Generate a platform.keystore from AOSP keys:
#     $ openssl pkcs8 -in platform.pk8 -inform DER -outform PEM > platform_key.pem
#     $ openssl pkcs12 -export -in platform.x509.pem -inkey platform_key.pem \
#         -out platform.p12 -name platform -password pass:android
#     $ keytool -importkeystore -destkeystore platform.keystore \
#         -srckeystore platform.p12 -srcstoretype pkcs12 \
#         -srcstorepass android -deststorepass android
#     Place platform.keystore + platform.x509.pem into
#     apps/android-collector/signing/ and create platform.properties.
#
# Usage:
#   ./scripts/ship-system.sh              # build + install + start
#   ./scripts/ship-system.sh --build-only # just build the APK
#   ./scripts/ship-system.sh --start-only # just start daemon + collector
#   ./scripts/ship-system.sh --uninstall  # remove system app
# ───────────────────────────────────────────────────────────────

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(cd "$APP_DIR/../.." && pwd)"
APK_NAME="app-platform.apk"
SYSTEM_PRIV_APP_DIR="/system/priv-app/DiPECSCollector"
DIPECSD_BIN="/system/bin/dipecsd"
TOKEN="${DIPECS_ANDROID_ACTION_BRIDGE_TOKEN:-dipecs-system-bridge-token-v2}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${GREEN}[ship]${NC} $*"; }
warn() { echo -e "${YELLOW}[ship]${NC} $*"; }
err()  { echo -e "${RED}[ship]${NC} $*" >&2; }

die() { err "$@"; exit 1; }

# ──────────────────────────────────────────────────
# Check ADB
# ──────────────────────────────────────────────────
check_adb() {
    if ! command -v adb &>/dev/null; then
        die "adb not found — install Android SDK platform-tools"
    fi
    if ! adb devices | grep -q -E 'device$'; then
        die "no ADB device connected"
    fi
}

check_root() {
    local whoami
    whoami="$(adb shell whoami 2>/dev/null || true)"
    if [ "$whoami" != "root" ]; then
        warn "ADB is not running as root. Attempting 'adb root'..."
        adb root || die "Cannot get root ADB — device must be rooted"
        sleep 2
    fi
}

# ──────────────────────────────────────────────────
# Build
# ──────────────────────────────────────────────────
build_apk() {
    log "Building platform-signed APK..."
    cd "$APP_DIR"
    ./gradlew :app:assemblePlatform -PDIPECS_PLATFORM_SIGNING=true || {
        warn "Platform build failed. Trying release build as fallback..."
        ./gradlew :app:assembleRelease || die "Gradle build failed"
        APK_NAME="app-release.apk"
    }
    cd "$REPO_ROOT"
    log "APK built."
}

build_dipecsd() {
    log "Cross-compiling dipecsd for aarch64-linux-android..."
    cd "$REPO_ROOT"
    # Load Android NDK linker config.
    if [ -f scripts/setup-env.sh ]; then
        source scripts/setup-env.sh 2>/dev/null || true
    fi
    cargo build --target aarch64-linux-android --release -p aios-daemon || die "dipecsd build failed"
    cd -
}

# ──────────────────────────────────────────────────
# Install
# ──────────────────────────────────────────────────
install_apk() {
    log "Remounting /system as read-write..."
    adb remount || warn "adb remount failed — is this an emulator or rooted device?"

    log "Pushing APK to $SYSTEM_PRIV_APP_DIR..."
    adb shell "mkdir -p $SYSTEM_PRIV_APP_DIR"
    adb push \
        "$APP_DIR/app/build/outputs/apk/platform/$APK_NAME" \
        "$SYSTEM_PRIV_APP_DIR/$APK_NAME" \
        || die "APK push failed"

    adb shell "chmod 644 $SYSTEM_PRIV_APP_DIR/$APK_NAME"
    log "APK installed as system priv-app."
}

install_dipecsd() {
    local dipecsd_path
    dipecsd_path="$REPO_ROOT/target/aarch64-linux-android/release/dipecsd"
    if [ ! -f "$dipecsd_path" ]; then
        die "dipecsd binary not found at $dipecsd_path — build it first"
    fi

    log "Pushing dipecsd to $DIPECSD_BIN..."
    adb push "$dipecsd_path" "$DIPECSD_BIN" || die "dipecsd push failed"
    adb shell "chmod 755 $DIPECSD_BIN"
    log "dipecsd installed."
}

set_token() {
    log "Setting bridge auth token..."
    adb shell "setprop persist.dipecs.bridge.token '$TOKEN'"
    log "Token set."
}

# ──────────────────────────────────────────────────
# Start / stop
# ──────────────────────────────────────────────────
start_daemon() {
    log "Starting dipecsd daemon..."
    # The daemon will fork; run in background from adb shell.
    # Env vars are set via setprop + init.rc or adb shell export.
    adb shell "DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=true \
               DIPECS_ANDROID_ACTION_BRIDGE_TOKEN='$TOKEN' \
               DIPECS_ANDROID_ACTION_BRIDGE_HOST=127.0.0.1 \
               DIPECS_ANDROID_ACTION_BRIDGE_PORT=46321 \
               $DIPECSD_BIN --no-daemon" &
    log "dipecsd started."
}

start_collector() {
    log "Starting DiPECS Collector system app..."
    # On a real device, the BootReceiver handles this. For dev, trigger manually.
    adb shell "am startservice \
        -n com.dipecs.collector/.services.CollectorForegroundService \
        -a com.dipecs.collector.action.START" \
        || warn "Could not start collector service (may already be running)"
}

stop_all() {
    log "Stopping DiPECS..."
    adb shell "killall dipecsd" 2>/dev/null || warn "dipecsd not running"
    adb shell "am stopservice \
        com.dipecs.collector/.services.CollectorForegroundService" \
        2>/dev/null || warn "Collector service not running"
}

uninstall() {
    log "Removing DiPECS system app..."
    adb remount
    adb shell "rm -rf $SYSTEM_PRIV_APP_DIR" || warn "Could not remove $SYSTEM_PRIV_APP_DIR"
    adb shell "rm -f $DIPECSD_BIN" || warn "Could not remove $DIPECSD_BIN"
    adb shell "reboot" || warn "Manual reboot required to clean up"
    log "Uninstalled. Reboot to apply."
}

# ──────────────────────────────────────────────────
# Verify
# ──────────────────────────────────────────────────
verify() {
    log "Verifying deployment..."
    echo ""
    echo "  dipecsd:"
    adb shell "$DIPECSD_BIN --version 2>/dev/null" || warn "  dipecsd not found or not executable"
    echo ""
    echo "  Socket server:"
    adb shell "ss -tlnp | grep 46321" 2>/dev/null || warn "  Port 46321 not listening"
    echo ""
    echo "  System app:"
    adb shell "pm list packages -f com.dipecs.collector | head -3" 2>/dev/null || warn "  APK not found"
    echo ""
    echo "  Bridge health check:"
    # Send a v2 ping to verify the bridge is alive.
    adb shell "echo '{\"message_type\":\"ping\"}' | timeout 2 nc 127.0.0.1 46321" 2>/dev/null \
        || warn "  Ping failed"
    echo ""
    log "Verification complete."
}

# ──────────────────────────────────────────────────
# Main
# ──────────────────────────────────────────────────
do_build=true
do_install=true
do_start=true
do_stop=false
do_uninstall=false

for arg in "$@"; do
    case "$arg" in
        --build-only) do_install=false; do_start=false ;;
        --start-only) do_build=false; do_install=false ;;
        --stop)       do_build=false; do_install=false; do_start=false; do_stop=true ;;
        --uninstall)  do_build=false; do_install=false; do_start=false; do_uninstall=true ;;
        --verify)     do_build=false; do_install=false; do_start=false; verify; exit 0 ;;
        -h|--help)
            echo "Usage: $0 [--build-only|--start-only|--stop|--uninstall|--verify]"
            exit 0
            ;;
        *) die "Unknown flag: $arg" ;;
    esac
done

if $do_uninstall; then
    check_adb; check_root
    uninstall
    exit 0
fi

if $do_stop; then
    check_adb
    stop_all
    exit 0
fi

if $do_build; then
    build_apk
    build_dipecsd
fi

if $do_install; then
    check_adb; check_root
    install_dipecsd
    install_apk
    set_token
    log "Rebooting to register system app..."
    adb reboot
    log "Wait for the device to reboot, then run: $0 --start-only"
fi

if $do_start; then
    check_adb; check_root
    start_collector
    sleep 2
    start_daemon
    sleep 2
    verify
fi
