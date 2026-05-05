# DiPECS Android Collector

This is the Kotlin-based Android phase-1 collector for DiPECS. It is a graphical interface-screening probe: enable one Android data source at a time, grant the matching interface permission, collect events, and inspect the JSONL trace samples before promoting a source into `aios-spec` / `aios-core`.

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

These toggles are meant for interface screening. Turn sources on/off, perform a target action on the device, then use the trace preview to decide whether that source is useful enough for the daemon/spec pipeline.

Events are stored as JSONL at:

```text
<app-private-files>/traces/actions.jsonl
```

Each JSONL row keeps the human-readable collector fields and, when a Rust-side schema exists, a `rawEvent` field using the same externally tagged JSON shape as `aios-spec::RawEvent`. Examples:

```json
{"rawEvent":{"AppTransition":{"timestamp_ms":0,"package_name":"com.android.chrome","activity_class":"MainActivity","transition":"Foreground"}}}
{"rawEvent":{"NotificationPosted":{"timestamp_ms":0,"package_name":"com.ss.android.lark","category":"msg","channel_id":"lark_im_message","raw_title":"张三","raw_text":"发来一个文件","is_ongoing":false,"group_key":"group","has_picture":false}}}
{"rawEvent":{"SystemState":{"timestamp_ms":0,"battery_pct":88,"is_charging":true,"network":"Wifi","ringer_mode":"Normal","location_type":"Unknown","headphone_connected":false,"bluetooth_connected":false}}}
```

The app can export the trace to its external files directory from the main screen.

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

The uploader sends the most recent 100 JSONL events as:

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

## Phase-1 Screening Workflow

1. Enable only one source, for example `UsageStatsManager`.
2. Start the collector and perform a small reproducible action.
3. Check the preview: source, event type, package, `raw=` kind, and text sample.
4. Export JSONL if the interface looks useful.
5. Clear the trace and repeat with the next source.

This keeps phase 1 focused on "what can this Android interface actually observe?" before the data model is hardened in Rust.
