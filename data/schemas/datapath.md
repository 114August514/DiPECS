# DiPECS Datapath — Data Schema Transformations

This document traces each transformation of the data schema from raw sensor input
to executed action and audit feedback. Each step is a pure type-to-type mapping
with deterministic rules — the JSON schema changes at each stage are described in
terms of what fields are added, dropped, or reshaped.

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

---

## Stage 4: Context Aggregation — `WindowAggregator`

**Code**: `aios-core/src/context_builder.rs:17`

**Input**: `Vec<SanitizedEvent>` accumulated over a time window (default 10s).

**Output**: `StructuredContext`

### `StructuredContext` schema

```json
{
  "window_id": "<UUID v4>",
  "window_start_ms": 1234567890000,
  "window_end_ms": 1234567900000,
  "duration_secs": 10,
  "events": [ /* SanitizedEvent[] — exact copies from Stage 3 */ ],
  "summary": { /* ContextSummary — new aggregated view */ }
}
```

### `ContextSummary` schema

```json
{
  "foreground_apps": ["com.android.chrome"],
  "notified_apps": ["com.whatsapp"],
  "all_semantic_hints": ["FileMention", "VerificationCode"],
  "file_activity": [["Document", 2], ["Image", 1]],
  "latest_system_status": {
    "battery_pct": 85,
    "is_charging": true,
    "network": "Wifi",
    "ringer_mode": "Vibrate",
    "location_type": "Home",
    "headphone_connected": false
  },
  "source_tier": "PublicApi"
}
```

### Aggregation rules

| Summary field | Derived from |
|---|---|
| `foreground_apps` | Unique `app_package` from `AppTransition(Foreground)`, `ProcessResource`, `InterAppInteraction` events |
| `notified_apps` | Unique `source_package` from `Notification` events |
| `all_semantic_hints` | Deduplicated `semantic_hints[]` from all `Notification` events in the window |
| `file_activity` | Counts per `ExtensionCategory` from `FileActivity` events |
| `latest_system_status` | The last `SystemStatus` event in the window (or null) |
| `source_tier` | `Daemon` if any event has `SourceTier::Daemon`, else `PublicApi` |

> The raw `events[]` remain unmodified alongside the summary — downstream backends may inspect individual events.

---

## Stage 5: Memory Enrichment — `ModelMemoryStore`

**Code**: `aios-core/src/context_memory.rs:76`

**Input**: `StructuredContext`

**Output**: `ModelInput`

### `ModelInput` schema

```json
{
  "current_context": { /* StructuredContext — unchanged */ },
  "behavior_profile": { /* UserBehaviorProfile — new from memory */ },
  "recent_feedback": [ /* RecentDecisionRecord[] — last N windows */ ]
}
```

### `UserBehaviorProfile` schema

```json
{
  "summary": "observed_windows=42; momentum_decay_milli=900; prewarm_effect_window=3; foreground_apps=com.chrome:28,com.whatsapp:12; notifying_apps=...",
  "observation_windows": 42,
  "frequent_foreground_apps": [["com.chrome", 28], ["com.whatsapp", 12]],
  "frequent_notifying_apps": [["com.whatsapp", 15]],
  "frequent_semantic_hints": [["FileMention", 8], ["UserMentioned", 5]],
  "action_successes": [["PreWarmProcess", 10], ["NoOp", 5]],
  "action_denials": [["PreWarmProcess", 3]],
  "action_failures": [["ReleaseMemory", 1]],
  "last_updated_window_id": "w-abc123"
}
```

**Momentum system**: Counts are decayed by `momentum_decay_milli/1000` each window. Top-N by momentum score is preferred; falls back to raw counts.

**LLM summary**: An optional `llm_summary=` prefix is prepended by the `ProfileSummaryWorker` (background thread at configurable interval). This is a privacy-preserving compressed natural-language summary of behavior trends.

### `RecentDecisionRecord` schema

```json
{
  "window_id": "w-xyz789",
  "window_start_ms": 1234567890000,
  "window_end_ms": 1234567900000,
  "foreground_apps": ["com.chrome"],
  "notified_apps": ["com.whatsapp"],
  "semantic_hints": ["FileMention"],
  "route": "CloudLlm",
  "model": "gpt-4o",
  "intent_count": 2,
  "rationale_tags": ["cloud_llm:model=gpt-4o"],
  "backend_error": null,
  "action_outcomes": [
    {
      "action_type": "PreWarmProcess",
      "target": "pkg:com.chrome",
      "terminal": "Succeeded",
      "correctness": "PredictionHit",
      "correctness_evidence": "prewarm target com.chrome opened within 1 observed window(s)",
      "denial_reason": null,
      "error": null,
      "outcome_summary": "prewarm_effect_hit:com.chrome"
    }
  ]
}
```

**Prewarm feedback**: When a `PreWarmProcess` action is executed and succeeds, a `PendingPrewarm` is registered. For up to `prewarm_effect_windows` subsequent windows, if the target package appears in `foreground_apps`, the original `ActionFeedbackRecord.correctness` is updated from `LikelyCorrect` → `PredictionHit`. If the observation window expires without a hit, it becomes `PredictionMiss`. This feedback loop updates the `recent_feedback` in-place.

---

## Stage 6: Decision Routing — `DecisionRouter`

**Code**: `aios-agent/src/router.rs:119`

**Input**: `ModelInput`

**Output**: `DecisionBackendResult`

### `DecisionBackendResult` schema

```json
{
  "route": "RuleBased" | "LocalEvaluator" | "CloudLlm" | "FallbackNoOp" | "Mock",
  "intent_batch": { /* IntentBatch — new */ },
  "rationale_tags": ["routing:medium_complexity(cloud_llm)"],
  "latency_us": 4200,
  "error": null
}
```

### `IntentBatch` schema

```json
{
  "window_id": "w-abc123",
  "intents": [
    {
      "intent_id": "<UUID v4>",
      "intent_type": "OpenApp(\"com.chrome\")" | "SwitchToApp(...)" | "CheckNotification(...)" | "HandleFile(Document)" | "EnterContext(...)" | "Idle",
      "confidence": 0.85,
      "risk_level": "Low" | "Medium" | "High",
      "suggested_actions": [
        {
          "action_type": "PreWarmProcess" | "PrefetchFile" | "KeepAlive" | "ReleaseMemory" | "NoOp",
          "target": "pkg:com.chrome",
          "urgency": "Immediate" | "IdleTime" | "Deferred"
        }
      ],
      "rationale_tags": ["cloud_llm:model=gpt-4o"]
    }
  ],
  "generated_at_ms": 1234567900000,
  "model": "gpt-4o"
}
```

### Routing decision tree

| Priority | Condition | Route |
|---|---|---|
| 1 | ≥5 consecutive backend errors within 60s | `FallbackNoOp` |
| 2 | Privacy score > 3 (VerificationCode/FinancialContext + AppTransition signals) | `RuleBased` |
| 2.5 | Local actionable signal (FileActivity, FileMention, ImageMention, LinkAttachment, AppTransition Foreground) | `LocalEvaluator` |
| 3a | 0-1 unique semantic hint types | `RuleBased` |
| 3b | 2-3 unique semantic hint types, cloud configured | `CloudLlm` |
| 3c | 2-3 unique semantic hint types, no cloud | `LocalEvaluator` |
| 3d | >3 unique semantic hint types, cloud configured | `CloudLlm` |
| 3e | >3 unique semantic hint types, no cloud | `LocalEvaluator` |

**Cloud fallback**: If `CloudLlm` backend returns an error, the router falls back to `RuleBased` and records the cloud error in `DecisionBackendResult.error`.

---

## Stage 7: Policy Evaluation — `PolicyEngine`

**Code**: `aios-core/src/policy_engine.rs:43`

**Input**: `IntentBatch` + `CapabilityLevel` + `StructuredContext`

**Output**: `Vec<PolicyActionDecision>`

This is **not a schema change** — it produces per-action verdicts consumed by the lifecycle state machine. But it's a gating step: rejected actions are blocked before reaching `ActionLifecycle`.

### Policy checks (in order for each intent + action)

| Check | Denial reason if failed |
|---|---|
| Intent risk exceeds `CapabilityLevel.max_risk` | `RiskExceedsCapability` |
| Intent risk exceeds `PolicyConfig.max_auto_risk` (default `Low`) | `RiskExceedsConfig` |
| Intent confidence < `PolicyConfig.min_confidence` (default 0.3) | `ConfidenceTooLow` |
| Action type matches blocked-action substring list | `ActionTypeBlocked` |
| Action urgency is `Deferred` | `ActionUrgencyDeferred` |
| Action type not in `CapabilityLevel.allowed_actions` | `ActionCapabilityDenied` |
| Target not in `StructuredContext`-derived known packages/paths | `TargetNotInContext` |
| Per-intent approved actions > `PolicyConfig.max_actions_per_batch` (default 5) | `BatchActionCapExceeded` |

### `CapabilityLevel` per route

| Route | `max_risk` | `allowed_actions` |
|---|---|---|
| `RuleBased` | `Low` | `NoOp`, `ReleaseMemory`, `KeepAlive` |
| `LocalEvaluator` | `Low` | All 5 action types |
| `CloudLlm` | `Medium` | All 5 action types |
| `FallbackNoOp` | `Low` | `NoOp` only |
| `Mock` | `Medium` | All 5 action types |

### `PolicyActionDecision` schema

```json
{
  "intent_ordinal": 0,
  "action_ordinal": 0,
  "verdict": "Approved" | { "Denied": "<DenialReason>" }
}
```

---

## Stage 8: Action Lifecycle — `ActionLifecycle`

**Code**: `aios-core/src/action_lifecycle.rs:36`

**Input**: `IntentBatch` + `DecisionRoute` + `CapabilityLevel` + `StructuredContext`

**Output**: `Vec<AuditRecord>` (one per action coordinate)

### Construction of `ActionProposal` (intermediate)

```json
{
  "intent_id": "<UUID from the intent>",
  "coord": { "window_ordinal": 0, "intent_ordinal": 0, "action_ordinal": 0 },
  "action": { /* SuggestedAction — copy from the intent */ },
  "effect": "PureRead" | "LocalCacheWrite" | "LocalStateChange",
  "proposed_at_ms": 1234567900000
}
```

`effect` is derived deterministically from `ActionType`:
- `NoOp` → `PureRead`
- `PrefetchFile` → `LocalCacheWrite`
- `PreWarmProcess`, `KeepAlive`, `ReleaseMemory` → `LocalStateChange`

### Lifecycle state machine

```
ActionProposal
  │
  ├─ [Schema validation] ── fail → RejectedInvalidSchema (terminal)
  │     │
  │     └─ pass → SchemaValidated
  │                   │
  │     [Policy lookup]──────── fail → PolicyChecked → DeniedByCapability (terminal)
  │                   │        fail → PolicyChecked → DeniedByPolicy (terminal)
  │                   │
  │                   └─ pass → PolicyChecked
  │                                │
  │                   [seal → AuthorizedAction]
  │                                │
  │                        [adapter.execute()]
  │                         │            │
  │                        ok            err
  │                     Succeeded      Failed
  │                    (terminal)    (terminal)
```

**Schema validation** checks:
- `PreWarmProcess` must have a non-empty `target`
- `PureRead` effect cannot carry `High` risk intent

### `AuthorizedAction` — sealed execution credential

```json
{
  "intent_id": "<UUID>",
  "coord": { "window_ordinal": 0, "intent_ordinal": 0, "action_ordinal": 0 },
  "action": { /* SuggestedAction */ },
  "effect": "LocalStateChange",
  "authorized_at_ms": 1234567900000
}
```

> `AuthorizedAction` is **sealed** — its constructor `pub(crate) seal()` is private to `aios-core`. It does **not** implement `Deserialize`. No external crate can fabricate one. It is the only type the `ActionAdapter` trait accepts.

---

## Stage 9: Action Execution — `ActionAdapter` implementations

**Code**: `aios-core/src/governance/mod.rs:65` (trait), `aios-action/src/` (implementations)

**Input**: `&AuthorizedAction`

**Output**: `Result<ActionOutcome, AdapterError>`

### `ActionOutcome` schema (success)

```json
{
  "action_type": "PreWarmProcess",
  "target": "pkg:com.chrome",
  "summary": "prefetched",
  "latency_us": 1234
}
```

### `AdapterError` variants

```json
"SimulatedResourceUnavailable(\"disk full\")"
"AndroidBridgeError(\"connection refused\")"
"ExecutionError(\"timeout\")"
```

### Adapter implementations

| Adapter | Behavior |
|---|---|
| `DefaultActionExecutor` (no-op stub) | Returns `Ok` for all actions with deterministic mock summary |
| `OfflineAdapter` | Deterministic replay stub (used in tests and golden traces) |
| `AndroidAdapter` | Serializes `AuthorizedAction` to JSON, sends via TCP to Android localhost bridge, returns bridge response |

### Android bridge wire protocol

**Request** (over TCP):
```json
{
  "message_type": "execute",
  "issued_at_ms": 1234567900000,
  "expires_at_ms": 1234567905000,
  "auth": {
    "hmac_sha256": "<hex HMAC over freshness window + action bytes>"
  },
  "action": "<canonical JSON of AuthorizedAction>"
}
```

**Response**:
```json
{
  "status": "ok" | "rejected" | "error",
  "summary": "prefetched",
  "latency_us": 1234,
  "error": null
}
```

---

## Stage 10: Audit Record — `AuditRecord`

**Code**: `aios-core/src/action_lifecycle.rs:55`

**Input**: aggregated from stages 7-9

**Output**: `Vec<AuditRecord>` — one per `ActionCoord`

### `AuditRecord` schema

```json
{
  "coord": { "window_ordinal": 0, "intent_ordinal": 0, "action_ordinal": 0 },
  "intent_id": "<UUID>",
  "action_type": "PreWarmProcess",
  "target": "pkg:com.chrome",
  "effect": "LocalStateChange",
  "route": "CloudLlm",
  "backend_error": null,
  "transitions": ["Proposed", "SchemaValidated", "PolicyChecked", "Dispatched", "Succeeded"],
  "terminal": "Succeeded",
  "outcome": { "action_type": "PreWarmProcess", "target": "pkg:com.chrome", "summary": "started" },
  "denial_reason": null,
  "error": null,
  "source_tier": "PublicApi"
}
```

| Field | Source | Deterministic? |
|---|---|---|
| `coord` | Derived from iteration order over batch | Yes |
| `intent_id` | Copied from `Intent` | No (UUID) |
| `action_type`, `target` | Copied from `SuggestedAction` | Yes |
| `effect` | Derived from `ActionType` | Yes |
| `route` | From `DecisionBackendResult` | Yes |
| `transitions` | Accumulated as lifecycle progresses | Yes |
| `terminal` | `ActionState` enum — final state | Yes |
| `outcome` | `ActionOutcomeSummary` (projected from `ActionOutcome`, drops `latency_us`) | Yes |
| `denial_reason` | `DenialReason` from `PolicyEngine` | Yes |
| `error` | `LifecycleError` or adapter `AdapterError` string | No |
| `source_tier` | From `ContextSummary.source_tier` | Yes |

---

## Stage 11: Feedback Loop — `ModelMemoryStore.observe_window()`

**Code**: `aios-core/src/context_memory.rs:247`

**Input**: `StructuredContext` + `DecisionBackendResult` + `&[AuditRecord]`

**Output**: Side effects on `ModelMemoryStore` (in-place mutation)

This closes the feedback loop:

1. **Prewarm resolution**: Pending prewarms from prior windows are checked against current `foreground_apps`. Hits/misses update correctness feedback on the originating `RecentDecisionRecord`.
2. **Momentum decay**: All momentum maps are multiplied by `momentum_decay_milli/1000`.
3. **Counter increments**: Foreground apps, notifying apps, semantic hints, and action outcomes (success/denial/failure) are counted and momentum-boosted.
4. **Recent decision push**: A new `RecentDecisionRecord` is pushed to the sliding window (max `recent_limit`, default 5).
5. **Prewarm registration**: Successful `PreWarmProcess` actions create `PendingPrewarm` entries for future hit/miss tracking.
6. **Profile summary polling**: On every `profile_summary_interval_windows` (default 10), a background thread compresses the behavior profile into an LLM summary.

---

## Golden Trace / Replay (Cross-cutting)

**Code**: `aios-spec/src/trace.rs`, `aios-core/src/trace_engine.rs`

### `GoldenTrace` schema

```json
{
  "trace_id": "<UUID>",
  "window_start_ms": 0,
  "window_end_ms": 10000,
  "raw_events": [ /* RawEvent[] — input */ ],
  "expected_sanitized": [ /* SanitizedEvent[] — expected output of Stage 3 */ ],
  "expected_intents": { /* IntentBatch — expected output of Stage 6 */ },
  "expected_actions": [ /* ExecutedAction[] — expected output of Stage 9 */ ],
  "source_tiers": [ /* SourceTier[] — parallel to raw_events */ ]
}
```

### `ReplayResult` schema

```json
{
  "trace_id": "<UUID>",
  "sanitization_match": true,
  "sanitization_divergences": [],
  "policy_match": false,
  "policy_divergences": ["intent at index 0: expected NoOp, got PreWarmProcess"],
  "execution_match": true,
  "execution_divergences": []
}
```

---

## Summary: Data Volume & Schema Evolution

```
Stage 0 (RawEvent)       PII-rich, 8 variant shapes, ~20-200 bytes per event
     │
     ▼ Stage 3 (airgap)
SanitizedEvent           6 variant shapes, PII-free, ~60-250 bytes per event
     │
     ▼ Stage 4 (window aggregate)
StructuredContext         events[] + summary, ~1-10 KB per window
     │
     ▼ Stage 5 (memory enrich)
ModelInput               context + profile + feedback, ~2-20 KB per window
     │
     ▼ Stage 6 (decision)
DecisionBackendResult    route + IntentBatch, ~100-2000 bytes per window
     │
     ▼ Stages 7-8 (policy + lifecycle)
AuditRecord[]            per-action coordinates + transitions, ~200-500 bytes per action
     │
     ▼ Stage 11 (feedback)
ModelMemoryStore         persistent counters/momentum, ~5-50 KB persisted to disk
```

**Key invariant**: After Stage 3, no JSON ever contains raw notification text, file paths,
notification keys, or group keys. All data crossing the decision backend boundary (Stages
5-6) is privacy-preserving.
