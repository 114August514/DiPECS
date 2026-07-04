# DiPECS Datapath — Decision & Policy (Stages 4–7)

> Part of the [Datapath schema series](datapath.md). This file covers context
> aggregation through policy evaluation: turning sanitized events into governed
> intents. Preceded by [Collection & Privacy (Stages 0–3)](datapath-collection.md);
> continues in [Execution & Feedback (Stages 8–11)](datapath-execution.md).

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
