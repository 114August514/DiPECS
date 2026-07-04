# DiPECS Datapath — Data Schema Transformations

This series traces each transformation of the data schema from raw sensor input
to executed action and audit feedback. Each step is a pure type-to-type mapping
with deterministic rules — the JSON schema changes at each stage are described in
terms of what fields are added, dropped, or reshaped.

The datapath is documented in three parts, following the pipeline's natural
boundaries. Each boundary drops information that must not cross it.

## Contents

### [Collection & Privacy — Stages 0–3](datapath-collection.md)

Raw collection through the privacy airgap: getting data in and making it safe.

- **Stage 0** — Raw Collection (`RawEvent` + `CollectorEnvelope`)
- **Stage 1** — Collector Ingress (`RustCollectorIngress`)
- **Stage 2** — Event Bus Transport (`ActionBus`)
- **Stage 3** — Privacy Airgap (`DefaultPrivacyAirGap`) — **the PII boundary**

### [Decision & Policy — Stages 4–7](datapath-decision.md)

Context aggregation through policy evaluation: turning sanitized events into
governed intents.

- **Stage 4** — Context Aggregation (`WindowAggregator`)
- **Stage 5** — Memory Enrichment (`ModelMemoryStore`)
- **Stage 6** — Decision Routing (`DecisionRouter`)
- **Stage 7** — Policy Evaluation (`PolicyEngine`)

### [Execution & Feedback — Stages 8–11](datapath-execution.md)

The action lifecycle through the feedback loop, plus cross-cutting golden-trace
schema and the end-to-end volume summary.

- **Stage 8** — Action Lifecycle (`ActionLifecycle`)
- **Stage 9** — Action Execution (`ActionAdapter` implementations)
- **Stage 10** — Audit Record (`AuditRecord`)
- **Stage 11** — Feedback Loop (`ModelMemoryStore.observe_window()`)
- **Golden Trace / Replay** (cross-cutting) and **Data Volume Summary**

## Pipeline at a glance

```
RawEvent ─airgap─▶ SanitizedEvent ─window─▶ StructuredContext ─memory─▶ ModelInput
   │ (Stage 0)        (Stage 3)              (Stage 4)                  (Stage 5)
   │                                                                       │
   │                                                                    decision
   │                                                                    (Stage 6)
   ▼                                                                       ▼
AuditRecord ◀─lifecycle─ AuthorizedAction ◀─policy─ IntentBatch ◀──────────┘
 (Stage 10)              (Stages 8–9)      (Stage 7)  (Stage 6)
   │
   ▼ feedback (Stage 11)
ModelMemoryStore
```

**Key invariant**: after Stage 3 (the airgap), no JSON ever contains raw
notification text, file paths, notification keys, or group keys. All data
crossing the decision backend boundary (Stages 5–6) is privacy-preserving.
