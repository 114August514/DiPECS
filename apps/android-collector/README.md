# DiPECS Android Collector

This is the Kotlin-based Android collector for DiPECS. It has two roles:

- Production bridge for Android public-API signals that already have
  `aios-spec::RawEvent` schemas.
- Interface-screening probe for optional sources that are still being evaluated.

## What It Collects

- Usage events from `UsageStatsManager`.
- Notification posted/removed events from `NotificationListenerService`.
- Window, click, focus, and text-change events from `AccessibilityService`.
- Device context snapshots: time zone, network, battery, screen state, ringer mode, and DND filter.

The main screen includes source toggles for:

- `UsageStatsManager`
- `NotificationListenerService`
- `AccessibilityService`
- `DeviceContext`

It also shows the current permission state, enabled-source state, trace size,
latest event timestamp, source/event distributions, production `rawEvent` row
count, `rawEvent: null` screening rows, JSON parse errors, collector service
state, last DeviceContext heartbeat, last export path, and action socket status.
These counters are intended for real-device validation: a source is not
considered useful until it produces non-null `rawEvent` rows that can be
replayed by Rust.

`UsageStatsManager`, `NotificationListenerService`, and `DeviceContext` are
production ingress sources when their events contain a non-null `rawEvent`.
`AccessibilityService` remains an optional screening/enhancement source until a
Rust-side schema is accepted for it.

Events are stored as JSONL at:

```text
<app-private-files>/traces/actions.jsonl
```

Each JSONL row keeps the human-readable collector fields and, when a Rust-side schema exists, a `rawEvent` field using the same externally tagged JSON shape as `aios-spec::RawEvent`. Examples:

```json
{"rawEvent":{"AppTransition":{"timestamp_ms":0,"package_name":"com.android.chrome","activity_class":"MainActivity","transition":"Foreground"}}}
{"rawEvent":{"NotificationPosted":{"timestamp_ms":0,"package_name":"com.chat.app","category":"msg","channel_id":"messages","raw_title":"Alice","raw_text":"sent a file","is_ongoing":false,"group_key":"group","has_picture":false}}}
{"rawEvent":{"SystemState":{"timestamp_ms":0,"battery_pct":88,"is_charging":true,"network":"Wifi","ringer_mode":"Normal","location_type":"Unknown","headphone_connected":false,"bluetooth_connected":false}}}
```

The `rawEvent` field is the Android-to-Rust production ingress format. The app
does not produce the final production context; Rust owns envelope validation,
privacy sanitization, window aggregation, and `StructuredContext` output.

The app can export the trace to its external files directory from the main
screen. Export and clear operations require explicit confirmation. Clear removes
both the local JSONL trace and prefetch cache. Exported
rows are sanitized before writing: notification title/text, accessibility text,
socket payloads, cache paths, and action targets are redacted.

After export, the main screen records the last export path and shows developer
commands for pulling the trace with `adb`, replaying it with `aios-cli`, running
`dipecsd --android-trace-jsonl`, forwarding the action socket port, and sending
a token-authenticated prefetch action.

For local inspection, open `tools/trace-dashboard/index.html` in a browser and
load the exported sanitized JSONL plus replay/audit NDJSON. The dashboard is
static and local-only; it summarizes event kinds, `rawEvent` coverage, replay
stages, and policy/audit decisions without rendering sensitive raw text.

When real-device data is not available, use the deterministic synthetic sample
instead:

```bash
python tools/generate_synthetic_android_trace.py --rows 2400
cargo run -p aios-cli -- replay data/traces/android_synthetic_large.redacted.jsonl \
  --stages policy \
  --audit data/evaluation/android_synthetic_large.audit.ndjson
```

This fixture is large enough for dashboard and replay demos, but every row is
marked `synthetic: true` and must not be described as a real-device capture.

## Rust Daemon Ingress

`dipecsd` can continuously consume an append-only Android trace file:

```bash
RUST_LOG=info cargo run -p aios-daemon --bin dipecsd -- --no-daemon --android-trace-jsonl path/to/actions.jsonl
```

Add `--trace-output path/to/runtime.ndjson` to persist one daemon window record
per line for audit/debug artifacts.

The same path can also be provided with:

```bash
DIPECS_ANDROID_TRACE_JSONL=path/to/actions.jsonl
DIPECS_RUNTIME_TRACE_OUTPUT=path/to/runtime.ndjson
```

Rows with `rawEvent: null` are skipped. Rows with a valid Rust `RawEvent`
shape are wrapped as `CollectorEnvelope` with `SourceTier::PublicApi` and sent
through the normal daemon pipeline.

## Emulator Automation

For Android Studio emulator validation on Windows, use the repository script:

```powershell
.\scripts\start-android-emulator.ps1
```

The script checks the Android SDK, installs the API 35 Google APIs x86_64 image
if needed, creates the `dipecs_emu` AVD, starts the emulator, waits for boot,
installs the debug APK, configures `adb forward tcp:46321 tcp:46321`, starts the
app, starts the debug collector through an adb-only debug activity, and pings
the action socket with a built-in TCP health check. Use `-Headless` for
CI-style runs or `-SkipHealthCheck` when you only need APK install/forwarding.
## Authorized Action Socket

The localhost action socket requires an `auth_token` field in every payload.
Release builds generate a random token on first launch and store it in
`EncryptedSharedPreferences`. The status panel only shows a redacted token; use
**Copy Action Socket Token** when you need to pass the release token to local tooling.

For Android Studio emulator validation, debug builds avoid the token bootstrap
chicken-and-egg problem. On first launch, if no token has been stored yet, the
app uses this fixed development token:

```bash
dipecs-dev-emulator-shared-token-00000000
```

The repository `.env.example` uses the same value:

```bash
DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=true
DIPECS_ANDROID_ACTION_BRIDGE_TOKEN=dipecs-dev-emulator-shared-token-00000000
```

You can override the debug token before the first app launch with adb:

```bash
adb shell setprop debug.dipecs.token my-local-debug-token
```

If the app has already generated or stored a token, clear app data or reinstall
before changing the debug token:

```bash
adb shell pm clear com.dipecs.collector
```

The CLI command is a ping/health-check and does not dispatch an action:

```bash
cargo run -p aios-cli -- send-authorized-action \
  --auth-token dipecs-dev-emulator-shared-token-00000000 \
  --host 127.0.0.1 \
  --port 46321
```

When `aios-action` forwards approved actions directly, set:

```bash
DIPECS_ANDROID_ACTION_BRIDGE_ENABLED=true
DIPECS_ANDROID_ACTION_BRIDGE_TOKEN=dipecs-dev-emulator-shared-token-00000000
```

Dispatched action payloads must include all of the following:

- `auth_token`, compared in constant time.
- `issued_at_ms` and `expires_at_ms`, with a maximum accepted TTL of 5 minutes.
- `action_signature`, an HMAC-SHA256 signature generated by `aios-action` over
  the action type, target, urgency, and freshness window.

Ping payloads are accepted with token auth only and never dispatch actions.

## Build

Open `apps/android-collector` in Android Studio, or run this from that directory:

```bash
./gradlew :app:assembleDebug
```

The local environment still needs Android SDK Platform 35 and Android Gradle Plugin access. The debug APK is written to:

```text
apps/android-collector/app/build/outputs/apk/debug/app-debug.apk
```

## CI

The GitHub Actions workflow at `.github/workflows/android-collector.yml` runs on changes under `apps/android-collector/**` and can also be triggered manually from the Actions page. It uses the Gradle wrapper to run:

```bash
./gradlew :app:testDebugUnitTest --stacktrace
./gradlew :app:assembleDebug --stacktrace
```

The workflow uploads the debug APK and unit test report as short-lived artifacts.

## Run

1. Install and open the app.
2. Grant Usage Access, Notification Listener access, Accessibility Service access, and Android 13+ notification runtime permission.
3. Enable the data sources you want to screen.
4. Set an upload endpoint and choose `mock` or `llm` mode.
5. Start the collector.
6. Switch apps, trigger notifications, and interact with UI elements.
7. Inspect the trace preview or export the JSONL trace.

Manual upload sends the most recent 100 sanitized JSONL events as:

```json
{
  "schema": "dipecs.collector.v1",
  "mode": "mock",
  "reason": "periodic",
  "generatedAtMs": 0,
  "events": []
}
```

In `llm` mode, the configured API key is sent as a bearer token. Upload failures are recorded as internal events and do not stop local collection.
Periodic upload is controlled by a separate **Enable periodic upload** switch
and is disabled by default. A configured endpoint alone is not enough to enable
background upload.

Network prefetch is restricted to `https://` targets, rejects local/private
addresses, and revalidates redirect targets. Upload endpoints must also use
`https://`, must not resolve to localhost/private/link-local/multicast
addresses, and are not followed across redirects.

Prefetch accepts `url:https://` and persisted `uri:content://` targets.
Prefetched bytes are capped at 2 MiB, stored under app cache, and automatically
cleaned after 24 hours. Socket-dispatched actions must include the action token,
HMAC signature, and short freshness window generated by the Rust `aios-action`
bridge.

Additional Android-safe actions are implemented with conservative local
semantics:

- `ReleaseMemory` accepts `cache:prefetch` or `cache:all` and only deletes
  DiPECS-owned cache files.
- `KeepAlive` accepts `work:*` targets and schedules a DiPECS-owned
  `JobScheduler` maintenance job.
- `PreWarmProcess` accepts `own:*` targets for DiPECS-owned resource warmup.
  `pkg:*` and `notif:*` targets post a user-visible notification hint instead
  of launching another app in the background.
- `NoOp` records an audit event and performs no action.

## Source Promotion Policy

Sources are considered promoted into the production chain only when all of the
following are true:

- The Android collector writes a Rust-compatible `rawEvent`.
- `aios-collector` parses it into `CollectorEnvelope` with `SourceTier::PublicApi`.
- `aios-core` sanitizes it without leaking raw text across `PrivacyAirGap`.
- Replay or daemon tests show the event participates in `StructuredContext`.

Currently promoted sources:

- `UsageStatsManager` -> `RawEvent::AppTransition`
- `NotificationListenerService` -> `RawEvent::NotificationPosted` /
  `RawEvent::NotificationInteraction`
- `DeviceContext` -> `RawEvent::SystemState`

Still screening:

- `AccessibilityService` events are recorded for investigation, but rows with
  `rawEvent: null` are skipped by the Rust production ingress.

`AccessibilityService` is disabled by default for new installs because it is a
high-sensitivity screening source. Enable it only when validating optional UI
signals and do not treat its rows as production ingress until a Rust schema is
accepted.

## Screening Workflow

1. Enable only one source, for example `UsageStatsManager`.
2. Start the collector and perform a small reproducible action.
3. Check the preview: source, event type, package, `raw=` kind, and text sample.
4. Export JSONL if the interface looks useful.
5. Clear the trace and prefetch cache, then repeat with the next source.

Use this workflow only for sources that are not yet promoted. Promoted sources
should be validated through `aios-cli replay` or `dipecsd --android-trace-jsonl`.
