//! JSONL replay — drive the core pipeline from an Android `CollectorEvent` trace.
//!
//! Each input line is the Android `CollectorEvent` JSON shape; we extract its
//! inner `rawEvent`, synthesize a `CollectorEnvelope`, and push it through
//! `RustCollectorIngress → DefaultPrivacyAirGap → WindowAggregator →
//! DecisionRouter → PolicyEngine`. Window boundaries use the captured
//! timestamps from the trace, not wall-clock time — replay is deterministic.
//!
//! Determinism is enforced by the **canonical audit stream**: every per-stage
//! record is also serialized into a sorted-key, volatility-stripped projection
//! that is both mirrored to an optional audit sink and folded into a SHA-256
//! hasher. The resulting hex digest (`audit_hash`) is pinned by golden tests:
//! any divergence in the pipeline's observable state transitions for a given
//! input trace is caught immediately.

use std::io::{BufRead, Write};

use aios_action::DefaultActionExecutor;
use aios_agent::DecisionRouter;
use aios_core::collector_ingress::RustCollectorIngress;
use aios_core::context_builder::WindowAggregator;
use aios_core::policy_engine::PolicyEngine;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_spec::traits::{ActionExecutor, PrivacySanitizer};
use aios_spec::{
    CapabilityLevel, CollectorEnvelope, IngestedRawEvent, RawEvent, SourceTier, StructuredContext,
};
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: &str = "dipecs.collector.v1";

/// Keys whose values are non-deterministic (uuids, wall-clock durations) and
/// must be stripped from the canonical audit projection so replay hashes are
/// stable across runs.
const VOLATILE_KEYS: &[&str] = &["event_id", "window_id", "intent_id", "latency_us"];

/// Pipeline stage at which replay should stop emitting events.
///
/// Higher stages imply that all preceding stages also run. `Policy` is the
/// default — it proves correctness through authorization without invoking the
/// (still-stubbed) action executor.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
pub enum Stage {
    Ingest,
    Sanitize,
    Context,
    Decision,
    Policy,
    Execute,
}

impl Stage {
    fn includes(self, other: Stage) -> bool {
        self >= other
    }
}

/// Aggregate counters surfaced both in the NDJSON summary record and to
/// integration tests.
///
/// `audit_hash` is a hex SHA-256 of the canonical projection of every
/// per-stage record (everything except the final summary record itself).
/// Identical inputs must yield identical hashes; pin this in golden tests.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct ReplaySummary {
    pub lines_total: u64,
    pub lines_skipped_no_raw_event: u64,
    pub lines_parse_error: u64,
    pub events_ingested: u64,
    pub windows_closed: u64,
    pub intents_total: u64,
    pub actions_authorized: u64,
    pub audit_hash: String,
}

/// Return value of [`run_with_audit`]: the summary (with the same `audit_hash`
/// as the field below) plus the hash hoisted for convenient assertions.
#[derive(Debug, Clone)]
pub struct ReplayWithAudit {
    pub summary: ReplaySummary,
    pub audit_hash: String,
}

/// Replay a JSONL stream through the core pipeline without writing a separate
/// audit file. The canonical-projection hash is still computed and surfaced as
/// `ReplaySummary.audit_hash` so callers can pin determinism without managing
/// an audit sink.
pub fn run<R: BufRead, W: Write>(
    reader: R,
    writer: &mut W,
    window_secs: u64,
    stage: Stage,
) -> Result<ReplaySummary> {
    let mut sink = std::io::sink();
    let with_audit = run_with_audit(reader, writer, &mut sink, window_secs, stage)?;
    Ok(with_audit.summary)
}

/// Replay a JSONL stream through the core pipeline, mirroring every per-stage
/// record into `audit` (volatility-stripped, sorted-key canonical form) and
/// returning the SHA-256 of that canonical stream.
///
/// The audit stream omits the trailing summary record so the hash itself can
/// be embedded into the summary without self-reference.
pub fn run_with_audit<R: BufRead, W: Write, A: Write>(
    reader: R,
    writer: &mut W,
    audit: &mut A,
    window_secs: u64,
    stage: Stage,
) -> Result<ReplayWithAudit> {
    let ingress = RustCollectorIngress;
    let sanitizer = DefaultPrivacyAirGap;
    let router = DecisionRouter::default();
    let policy = PolicyEngine::default();
    let executor = DefaultActionExecutor;
    let mut summary = ReplaySummary::default();
    let mut aggregator: Option<WindowAggregator> = None;
    let mut last_captured_at_ms: i64 = 0;
    let mut emitter = Emitter::new(writer, audit);

    for (line_idx, line_result) in reader.lines().enumerate() {
        let line_no = line_idx as u64 + 1;
        let raw_line = line_result.with_context(|| format!("read line {line_no}"))?;
        if raw_line.trim().is_empty() {
            continue;
        }
        summary.lines_total += 1;

        let line_value: Value = match serde_json::from_str(&raw_line) {
            Ok(v) => v,
            Err(e) => {
                summary.lines_parse_error += 1;
                emitter.emit(&json!({
                    "stage": "error",
                    "line": line_no,
                    "error": format!("invalid JSON: {e}"),
                }))?;
                continue;
            },
        };

        let raw_event_value = line_value.get("rawEvent").cloned().unwrap_or(Value::Null);
        if raw_event_value.is_null() {
            summary.lines_skipped_no_raw_event += 1;
            continue;
        }

        let raw_event: RawEvent = match serde_json::from_value(raw_event_value) {
            Ok(r) => r,
            Err(e) => {
                summary.lines_parse_error += 1;
                emitter.emit(&json!({
                    "stage": "error",
                    "line": line_no,
                    "error": format!("rawEvent does not match RawEvent shape: {e}"),
                }))?;
                continue;
            },
        };

        let envelope = CollectorEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            source: line_value
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("apps.android-collector.replay")
                .to_string(),
            source_tier: SourceTier::PublicApi,
            device_trace_id: line_value
                .get("eventId")
                .and_then(Value::as_str)
                .map(String::from),
            captured_at_ms: line_value
                .get("timestampMs")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            received_at_ms: None,
            raw_event,
        };
        let captured_at_ms = envelope.captured_at_ms;
        last_captured_at_ms = last_captured_at_ms.max(captured_at_ms);

        let ingested: IngestedRawEvent = ingress
            .accept(envelope)
            .with_context(|| format!("ingress.accept failed on line {line_no}"))?;
        summary.events_ingested += 1;

        if stage.includes(Stage::Ingest) {
            emitter.emit(&json!({
                "stage": "ingest",
                "line": line_no,
                "source_tier": format!("{:?}", ingested.source_tier),
                "raw_event_kind": raw_event_kind(&ingested.raw_event),
            }))?;
        }

        // Time-based window driven by the trace's own timestamps.
        let agg =
            aggregator.get_or_insert_with(|| WindowAggregator::new(window_secs, captured_at_ms));
        if agg.is_expired(captured_at_ms) {
            if let Some(ctx) = agg.close(captured_at_ms) {
                process_window(
                    &ctx,
                    &router,
                    &policy,
                    &executor,
                    stage,
                    &mut summary,
                    &mut emitter,
                )?;
            }
        }

        if !stage.includes(Stage::Sanitize) {
            continue;
        }
        let sanitized = sanitizer.sanitize_with_tier(ingested.raw_event, ingested.source_tier);
        emitter.emit(&json!({
            "stage": "sanitize",
            "line": line_no,
            "sanitized": sanitized,
        }))?;
        if stage.includes(Stage::Context) {
            // Push into the aggregator so the next window close can build context.
            if let Some(agg) = aggregator.as_mut() {
                agg.push(sanitized);
            }
        }
    }

    // Flush the last open window using the latest captured timestamp seen.
    if let Some(mut agg) = aggregator {
        if let Some(ctx) = agg.close(last_captured_at_ms) {
            process_window(
                &ctx,
                &router,
                &policy,
                &executor,
                stage,
                &mut summary,
                &mut emitter,
            )?;
        }
    }

    let audit_hash = emitter.finalize();
    summary.audit_hash = audit_hash.clone();

    // Summary record goes only to the human-facing writer — it contains the
    // hash itself and therefore must not be folded back into the hash.
    serde_json::to_writer(
        &mut *writer,
        &json!({
            "stage": "summary",
            "summary": summary,
        }),
    )?;
    writer.write_all(b"\n")?;

    Ok(ReplayWithAudit {
        summary,
        audit_hash,
    })
}

fn process_window(
    ctx: &StructuredContext,
    router: &DecisionRouter,
    policy: &PolicyEngine,
    executor: &DefaultActionExecutor,
    stage: Stage,
    summary: &mut ReplaySummary,
    emitter: &mut Emitter<'_>,
) -> Result<()> {
    summary.windows_closed += 1;

    if stage.includes(Stage::Context) {
        emitter.emit(&json!({
            "stage": "context",
            "window_id": ctx.window_id,
            "window_start_ms": ctx.window_start_ms,
            "window_end_ms": ctx.window_end_ms,
            "duration_secs": ctx.duration_secs,
            "event_count": ctx.events.len(),
            "summary": ctx.summary,
        }))?;
    }

    if !stage.includes(Stage::Decision) {
        return Ok(());
    }
    let decision = router.evaluate(ctx);
    summary.intents_total += decision.intent_batch.intents.len() as u64;
    emitter.emit(&json!({
        "stage": "decision",
        "window_id": ctx.window_id,
        "route": format!("{:?}", decision.route),
        "model": decision.intent_batch.model,
        "intent_count": decision.intent_batch.intents.len(),
        "rationale_tags": decision.rationale_tags,
        "error": decision.error,
    }))?;

    if !stage.includes(Stage::Policy) {
        return Ok(());
    }
    let capability = CapabilityLevel::for_route(decision.route);
    let decisions = policy.evaluate_batch_with_capability(&decision.intent_batch, &capability);
    for d in &decisions {
        if d.approved {
            summary.actions_authorized += d.approved_actions.len() as u64;
        }
        emitter.emit(&json!({
            "stage": "policy",
            "window_id": ctx.window_id,
            "intent_id": d.intent_id,
            "approved": d.approved,
            "rejection_reason": d.rejection_reason,
            "capability_denials": d.capability_denials,
            "approved_actions": d.approved_actions,
        }))?;
    }

    if !stage.includes(Stage::Execute) {
        return Ok(());
    }
    for d in &decisions {
        if !d.approved {
            continue;
        }
        let results = executor.execute_batch(&d.approved_actions);
        for r in &results {
            emitter.emit(&json!({
                "stage": "execute",
                "window_id": ctx.window_id,
                "intent_id": d.intent_id,
                "action_type": r.action_type,
                "success": r.success,
                "error": r.error,
            }))?;
        }
    }
    Ok(())
}

/// Twin sink: every record goes to the human-facing `writer` verbatim and to
/// the `audit` sink in canonical (sorted-key, volatility-stripped) form, while
/// the canonical bytes are also folded into a SHA-256 hasher for the replay
/// fingerprint.
struct Emitter<'a> {
    writer: &'a mut dyn Write,
    audit: &'a mut dyn Write,
    hasher: Sha256,
}

impl<'a> Emitter<'a> {
    fn new<W: Write, A: Write>(writer: &'a mut W, audit: &'a mut A) -> Self {
        Self {
            writer,
            audit,
            hasher: Sha256::new(),
        }
    }

    fn emit(&mut self, value: &Value) -> Result<()> {
        serde_json::to_writer(&mut *self.writer, value)?;
        self.writer.write_all(b"\n")?;
        let canonical = canonicalize(value);
        let canonical_bytes = serde_json::to_vec(&canonical)?;
        self.audit.write_all(&canonical_bytes)?;
        self.audit.write_all(b"\n")?;
        self.hasher.update(&canonical_bytes);
        self.hasher.update(b"\n");
        Ok(())
    }

    fn finalize(self) -> String {
        format!("sha256:{:x}", self.hasher.finalize())
    }
}

/// Recursively rebuild `value` so that:
/// - object keys are sorted (canonical order),
/// - any key listed in [`VOLATILE_KEYS`] is dropped.
///
/// This is the only projection ever fed into the audit hasher; trace-derived
/// timestamps (`window_start_ms`, `generated_at_ms`, etc.) are intentionally
/// kept so they participate in the determinism check.
fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for k in keys {
                if VOLATILE_KEYS.contains(&k.as_str()) {
                    continue;
                }
                if let Some(v) = map.get(k) {
                    out.insert(k.clone(), canonicalize(v));
                }
            }
            Value::Object(out)
        },
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

fn raw_event_kind(raw: &RawEvent) -> &'static str {
    match raw {
        RawEvent::AppTransition(_) => "AppTransition",
        RawEvent::BinderTransaction(_) => "BinderTransaction",
        RawEvent::ProcStateChange(_) => "ProcStateChange",
        RawEvent::FileSystemAccess(_) => "FileSystemAccess",
        RawEvent::NotificationPosted(_) => "NotificationPosted",
        RawEvent::NotificationInteraction(_) => "NotificationInteraction",
        RawEvent::ScreenState(_) => "ScreenState",
        RawEvent::SystemState(_) => "SystemState",
    }
}
