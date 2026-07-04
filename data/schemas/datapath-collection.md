# DiPECS Datapath — Collection & Privacy (Stages 0–3)

> Part of the [Datapath schema series](datapath.md). This file covers raw
> collection through the privacy airgap: getting data in and making it safe.
> Continues in [Decision & Policy (Stages 4–7)](datapath-decision.md) and
> [Execution & Feedback (Stages 8–11)](datapath-execution.md).

Each step is a pure type-to-type mapping with deterministic rules — the JSON
schema changes at each stage are described in terms of what fields are added,
dropped, or reshaped.

---

## Stage 0: Raw Collection — `RawEvent` + `CollectorEnvelope`

**Source**: Android sensors (UsageStatsManager, eBPF, /proc, fanotify, NotificationListenerService) + system state polling.

**Output type**: `CollectorEnvelope` (external / app-sourced) or `RawEvent` (internal Rust collectors).

### `CollectorEnvelope` (external ingress)
```json
{
  "schema_version": "dipecs.collector.v1",
  "source": "<collector identifier>",
  "source_tier": "PublicApi" | "Daemon" | "PrivilegedDaemon" | "SystemImage",
  "device_trace_id": "<optional trace UUID>",
  "captured_at_ms": 1234567890000,
  "received_at_ms": null,
  "raw_event": { /* one of the RawEvent variants below */ }
}
```

### `RawEvent` (enum — all variants shown schema-side)

| Variant | Key fields |
|---|---|
| `AppTransition` | `timestamp_ms`, `package_name`, `activity_class`?, `transition: Foreground\|Background` |
| `BinderTransaction` | `timestamp_ms`, `source_pid`, `source_uid`, `target_service`, `target_method`, `is_oneway`, `payload_size` |
| `ProcStateChange` | `timestamp_ms`, `pid`, `uid`, `package_name`?, `vm_rss_kb`, `vm_swap_kb`, `threads`, `oom_score`, `io_read_mb`, `io_write_mb`, `state: Running\|Sleeping\|Zombie\|Unknown` |
| `FileSystemAccess` | `timestamp_ms`, `pid`, `uid`, `file_path` **(PII — file path, dropped at airgap)**, `access_type`, `bytes_transferred`? |
| `NotificationPosted` | `timestamp_ms`, `package_name`, `category`?, `channel_id`?, `raw_title` **(PII)**, `raw_text` **(PII)**, `title_hint`?, `text_hint`?, `semantic_hints[]`, `is_ongoing`, `group_key`?, `has_picture` |
| `NotificationInteraction` | `timestamp_ms`, `package_name`, `notification_key`, `action: Tapped\|Dismissed\|Cancelled\|Seen` |
| `ScreenState` | `timestamp_ms`, `state: Interactive\|NonInteractive\|KeyguardShown\|KeyguardHidden` |
| `SystemState` | `timestamp_ms`, `battery_pct`?, `is_charging`, `network`, `ringer_mode`, `location_type`, `headphone_connected`, `bluetooth_connected` |

> **PII note**: `raw_title`, `raw_text`, `file_path`, `notification_key`, `group_key` contain raw user data. They exist **only** in this stage and are destroyed by the airgap in Stage 3.

---

## Stage 1: Collector Ingress — `RustCollectorIngress`

**Code**: `aios-core/src/collector_ingress.rs:14`

**Input**: `CollectorEnvelope` (external) or `(RawEvent, source_label, captured_at_ms)` (internal).

**Output**: `IngestedRawEvent`

### Schema transform

```json
{
  "raw_event": "<RawEvent (unchanged)>",
  "source_tier": "<authoritative SourceTier from the ingress claim>"
}
```

| Change | Detail |
|---|---|
| Added | `source_tier` — authoritative origin tier, set by the ingress method (`PublicApi` for Android envelopes, `Daemon` for Rust collectors) |
| Dropped | `schema_version`, `source`, `device_trace_id`, `captured_at_ms`, `received_at_ms` — envelope metadata consumed and discarded |
| Validated | `schema_version` must be `"dipecs.collector.v1"` or the envelope is rejected |

> `IngestedRawEvent` now travels via tokio mpsc (`ActionBus.raw_sender()` → `ActionBus.recv_raw()`) to the processing task. The `source_tier` rides alongside the event for the rest of the pipeline.

---

## Stage 2: Event Bus Transport — `ActionBus`

**Code**: `aios-core/src/action_bus.rs`

**Input**: `IngestedRawEvent`

**Output**: `IngestedRawEvent` (identity — just an mpsc channel transfer)

No schema change. This is a pure transport layer.

> The bus is a bounded `tokio::sync::mpsc::channel<IngestedRawEvent>(4096)`.

---

## Stage 3: Privacy Airgap — `DefaultPrivacyAirGap`

**Code**: `aios-core/src/privacy_airgap.rs:16`

**Input**: `RawEvent` + `SourceTier`

**Output**: `SanitizedEvent`

This is the **most important transformation**. All PII is stripped; the output can be freely transmitted to cloud backends.

### `SanitizedEvent` schema

```json
{
  "event_id": "<UUID v4>",
  "timestamp_ms": 1234567890000,
  "event_type": "<SanitizedEventType (see below)>",
  "source_tier": "PublicApi" | "Daemon",
  "app_package": "<package name or null>",
  "uid": 10086
}
```

### `RawEvent` → `SanitizedEvent` mapping rules

| RawEvent variant | SanitizedEventType | PII dropped | Derived fields |
|---|---|---|---|
| `AppTransition` | `AppTransition { package_name, activity_class, transition }` | — | `app_package` copied from `package_name` |
| `BinderTransaction` | `InterAppInteraction { source_package, target_service, interaction_type }` | `source_pid`, `source_uid` (moved to `uid` field), `payload_size` | `interaction_type` inferred from `target_method` substring match: `enqueueNotification → NotifyPost`, `startActivity → ActivityLaunch`, `share → ShareIntent`, `bindService → ServiceBind` |
| `ProcStateChange` | `ProcessResource { pid, package_name, vm_rss_mb, vm_swap_mb, thread_count, oom_score }` | `uid` (moved to `uid` field), `io_read_mb`, `io_write_mb`, `state` | `vm_rss_kb/1024 → vm_rss_mb`, `vm_swap_kb/1024 → vm_swap_mb`, `threads → thread_count` |
| `FileSystemAccess` | `FileActivity { package_name, extension_category, activity_type, is_hot_file }` | **`file_path`** (only extension used), `pid`, `uid`, `bytes_transferred` | `extension_category` from file extension (`.docx → Document`, `.jpg → Image`, etc.), `is_hot_file` from stats tracking |
| `NotificationPosted` | `Notification { source_package, category, channel_id, title_hint, text_hint, semantic_hints[], is_ongoing, group_key }` | **`raw_title`** (replaced by `title_hint`), **`raw_text`** (replaced by `text_hint`), `has_picture`, `group_key` | `title_hint`: `{ length_chars, script: Latin\|Hanzi\|...\|Unknown, is_emoji_only }`; `text_hint`: same shape; `semantic_hints`: keyword match from raw text (FileMention, VerificationCode, FinancialContext, etc.) |
| `NotificationInteraction` | `Notification { source_package, ... }` — all empty hints | **`notification_key`** (includes PII tag), `action` | All hints set to empty/zero defaults |
| `ScreenState` | `Screen { state }` | — | Direct field copy |
| `SystemState` | `SystemStatus { battery_pct, is_charging, network, ringer_mode, location_type, headphone_connected }` | `bluetooth_connected` | Direct field copy |

> **Text hints** (`title_hint`, `text_hint`) were already computed at the collector side and live on `RawEvent::NotificationPosted` directly. This means even the **raw event** never transmits full text if the collector pre-computes the hints — the `raw_title`/`raw_text` fields are stripped immediately at the airgap regardless.

### After this stage

- **No** `NotificationPosted.raw_title` or `.raw_text`
- **No** `NotificationInteraction.notification_key`
- **No** `FileSystemAccess.file_path`
- **No** `GroupKey` (dropped: is PII-containing Android key)
- All package names, `pid`, `uid` are **retained** (not PII under Android privacy model)
