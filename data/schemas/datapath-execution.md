# DiPECS Datapath — Execution & Feedback (Stages 8–11)

> Part of the [Datapath schema series](datapath.md). This file covers the
> action lifecycle through the feedback loop, plus the cross-cutting golden
> trace / replay schema and the end-to-end data-volume summary. Preceded by
> [Collection & Privacy (Stages 0–3)](datapath-collection.md) and
> [Decision & Policy (Stages 4–7)](datapath-decision.md).

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
