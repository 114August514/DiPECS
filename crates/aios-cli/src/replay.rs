//! JSONL replay — drive the core pipeline from an Android `CollectorEvent` trace.
//!
//! Each input line is the Android `CollectorEvent` JSON shape; we extract its
//! inner `rawEvent`, synthesize a `CollectorEnvelope`, and push it through
//! `RustCollectorIngress → DefaultPrivacyAirGap → WindowAggregator →
//! DecisionRouter → ActionLifecycle`. Window boundaries use the captured
//! timestamps from the trace, not wall-clock time — replay is deterministic.
//!
//! Determinism is enforced by the **canonical audit stream**: every per-stage
//! record (including `AuditRecord` from the Action Bus lifecycle) is also
//! serialized into a sorted-key, volatility-stripped projection that is both
//! mirrored to an optional audit sink and folded into a SHA-256 hasher. The
//! resulting hex digest (`audit_hash`) is pinned by golden tests: any
//! divergence in the pipeline's observable state transitions for a given
//! input trace is caught immediately.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};

use aios_action::OfflineAdapter;
use aios_agent::DecisionRouter;
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::collector_ingress::RustCollectorIngress;
use aios_core::context_builder::WindowAggregator;
use aios_core::policy_engine::PolicyEngine;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_spec::governance::{ActionState, AuditRecord};
use aios_spec::traits::PrivacySanitizer;
use aios_spec::{
    CapabilityLevel, CollectorEnvelope, DenialReason, IngestedRawEvent, RawEvent, SourceTier,
    StructuredContext,
};
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: &str = "dipecs.collector.v1";

/// Keys whose values are non-deterministic (uuids, wall-clock durations) and
/// must be stripped from the canonical audit projection so replay hashes are
/// stable across runs.
const VOLATILE_KEYS: &[&str] = &[
    "event_id",
    "window_id",
    "intent_id",
    "latency_us",
    "backend_error",
];

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
///
/// `denial_counts` keys are `DenialReason` enum variants; the `BTreeMap`
/// gives a stable, canonical iteration order so the JSON projection is
/// deterministic and folds into `audit_hash` without extra effort.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct ReplaySummary {
    pub lines_total: u64,
    pub lines_skipped_no_raw_event: u64,
    pub lines_parse_error: u64,
    pub events_ingested: u64,
    pub windows_closed: u64,
    pub intents_total: u64,
    pub intents_approved: u64,
    pub intents_rejected: u64,
    pub actions_authorized: u64,
    pub actions_denied: u64,
    pub actions_failed: u64,
    pub audit_records: u64,
    pub denial_counts: BTreeMap<DenialReason, u64>,
    pub audit_hash: String,
}

/// Return value of [`run_with_audit`]: the summary (with the same `audit_hash`
/// as the field below) plus the hash hoisted for convenient assertions.
#[derive(Debug, Clone)]
pub struct ReplayWithAudit {
    pub summary: ReplaySummary,
    pub audit_hash: String,
}

impl ReplaySummary {
    /// Render a compact, eyeball-friendly summary for stderr. The canonical
    /// machine view is the `{"stage":"summary",...}` NDJSON record already on
    /// the output sink; this view trades precision for scannability.
    ///
    /// Layout is stable: pipeline / intents / actions blocks are always
    /// emitted; `denials by reason` only appears when `denial_counts` is
    /// non-empty. The audit hash is truncated to the first 16 hex chars after
    /// the `sha256:` prefix — enough to eyeball-diff against a pinned value.
    pub fn human_summary(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(out, "=== DiPECS replay ===");
        let _ = writeln!(out, "audit    {}", short_hash(&self.audit_hash));
        let _ = writeln!(
            out,
            "input    {} lines · {} ingested · {} skipped · {} parse errors",
            self.lines_total,
            self.events_ingested,
            self.lines_skipped_no_raw_event,
            self.lines_parse_error,
        );
        let _ = writeln!(out, "windows  {} closed", self.windows_closed);
        out.push('\n');
        let _ = writeln!(
            out,
            "intents  {} total · {} approved · {} rejected",
            self.intents_total, self.intents_approved, self.intents_rejected,
        );
        let _ = writeln!(
            out,
            "actions  {} authorized · {} denied · {} failed · {} audit records",
            self.actions_authorized, self.actions_denied, self.actions_failed, self.audit_records,
        );

        if !self.denial_counts.is_empty() {
            out.push('\n');
            let _ = writeln!(out, "denials by reason");
            let name_width = self
                .denial_counts
                .keys()
                .map(|r| format!("{r:?}").len())
                .max()
                .unwrap_or(0);
            for (reason, count) in &self.denial_counts {
                let _ = writeln!(
                    out,
                    "  {name:<width$}  {count}",
                    name = format!("{reason:?}"),
                    width = name_width,
                );
            }
        }
        out
    }
}

fn short_hash(audit_hash: &str) -> String {
    if audit_hash.is_empty() {
        return "(none)".into();
    }
    if let Some(hex) = audit_hash.strip_prefix("sha256:") {
        if hex.len() > 16 {
            return format!("sha256:{}…  (full hash in NDJSON summary)", &hex[..16]);
        }
    }
    audit_hash.to_string()
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
    let adapter = OfflineAdapter;
    let lifecycle = ActionLifecycle::new(&policy, &adapter);
    let mut summary = ReplaySummary::default();
    let mut aggregator: Option<WindowAggregator> = None;
    let mut last_captured_at_ms: i64 = 0;
    let mut emitter = Emitter::new(writer, audit);
    let mut window_ordinal = 0u32;

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
            let source_tier = &ingested.source_tier;
            emitter.emit(&json!({
                "stage": "ingest",
                "line": line_no,
                "source_tier": format!("{source_tier:?}"),
                "raw_event_kind": raw_event_kind(&ingested.raw_event),
            }))?;
        }

        // Time-based window driven by the trace's own timestamps.
        let agg =
            aggregator.get_or_insert_with(|| WindowAggregator::new(window_secs, captured_at_ms));
        if agg.is_expired(captured_at_ms) {
            if let Some(ctx) = agg.close(captured_at_ms) {
                process_window(
                    window_ordinal,
                    &ctx,
                    &router,
                    &lifecycle,
                    stage,
                    &mut summary,
                    &mut emitter,
                )?;
                window_ordinal += 1;
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
                window_ordinal,
                &ctx,
                &router,
                &lifecycle,
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
    window_ordinal: u32,
    ctx: &StructuredContext,
    router: &DecisionRouter,
    lifecycle: &ActionLifecycle,
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
    let route = &decision.route;
    emitter.emit(&json!({
        "stage": "decision",
        "window_id": ctx.window_id,
        "route": format!("{route:?}"),
        "model": decision.intent_batch.model,
        "intent_count": decision.intent_batch.intents.len(),
        "rationale_tags": decision.rationale_tags,
        "error": decision.error.clone(),
    }))?;

    if !stage.includes(Stage::Policy) {
        return Ok(());
    }
    let capability = CapabilityLevel::for_route(decision.route);

    if stage.includes(Stage::Execute) {
        // Full lifecycle: schema -> policy -> seal -> adapter -> terminal audit.
        let audit_records = lifecycle.run(
            window_ordinal,
            &decision.intent_batch,
            decision.route,
            decision.error.clone(),
            &capability,
            ctx,
        );

        // Aggregate per-intent signals for the human-facing summary.
        // An intent is "approved" once policy authorizes at least one of its
        // actions, regardless of whether the downstream adapter later fails.
        // The `Failed` terminal is therefore counted as authorized *and* failed.
        let mut approved_intents = 0u64;
        let mut rejected_intents = 0u64;
        for (intent_idx, _intent) in decision.intent_batch.intents.iter().enumerate() {
            let intent_ordinal = intent_idx as u32;
            let intent_records: Vec<&AuditRecord> = audit_records
                .iter()
                .filter(|r| r.coord.intent_ordinal == intent_ordinal)
                .collect();
            let any_authorized = intent_records
                .iter()
                .any(|r| matches!(r.terminal, ActionState::Succeeded | ActionState::Failed));
            if any_authorized {
                approved_intents += 1;
            } else {
                rejected_intents += 1;
            }
        }
        summary.intents_approved += approved_intents;
        summary.intents_rejected += rejected_intents;

        for record in &audit_records {
            summary.audit_records += 1;
            match record.terminal {
                ActionState::Succeeded | ActionState::Failed => {
                    summary.actions_authorized += 1;
                },
                ActionState::RejectedInvalidSchema
                | ActionState::DeniedByCapability
                | ActionState::DeniedByPolicy => {
                    summary.actions_denied += 1;
                    if let Some(reason) = record.denial_reason {
                        *summary.denial_counts.entry(reason).or_insert(0) += 1;
                    }
                },
                _ => {},
            }
            if matches!(record.terminal, ActionState::Failed) {
                summary.actions_failed += 1;
            }
        }

        emitter.emit(&json!({
            "stage": "policy",
            "window_id": ctx.window_id,
            "window_ordinal": window_ordinal,
            "intent_count": decision.intent_batch.intents.len(),
            "audit_records": audit_records,
        }))?;

        for record in &audit_records {
            let action_type = &record.action_type;
            emitter.emit(&json!({
                "stage": "execute",
                "window_id": ctx.window_id,
                "window_ordinal": window_ordinal,
                "coord": record.coord,
                "intent_id": record.intent_id,
                "action_type": format!("{action_type:?}"),
                "terminal": record.terminal,
                "outcome": record.outcome,
                "error": record.error,
            }))?;
        }
        return Ok(());
    }

    // Policy-only stage: evaluate without executing, preserving old summary semantics.
    let policy_decisions =
        lifecycle
            .policy()
            .evaluate_batch_with_context(&decision.intent_batch, &capability, ctx);

    let mut by_intent: std::collections::BTreeMap<
        u32,
        Vec<&aios_spec::governance::PolicyActionDecision>,
    > = std::collections::BTreeMap::new();
    for d in &policy_decisions {
        by_intent.entry(d.intent_ordinal).or_default().push(d);
    }

    for (intent_idx, intent) in decision.intent_batch.intents.iter().enumerate() {
        let intent_ordinal = intent_idx as u32;
        let intent_decisions = by_intent
            .get(&intent_ordinal)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let approved_count = intent_decisions
            .iter()
            .filter(|d| matches!(d.verdict, aios_spec::governance::PolicyVerdict::Approved))
            .count() as u64;
        let denied_count = intent_decisions.len() as u64 - approved_count;

        if approved_count > 0 {
            summary.intents_approved += 1;
        } else {
            summary.intents_rejected += 1;
        }
        summary.actions_authorized += approved_count;
        summary.actions_denied += denied_count;

        let mut action_denials = Vec::new();
        let mut approved_actions = Vec::new();
        for d in intent_decisions {
            match d.verdict {
                aios_spec::governance::PolicyVerdict::Approved => {
                    if let Some(action) = intent.suggested_actions.get(d.action_ordinal as usize) {
                        approved_actions.push(json!({ "action": action }));
                    }
                },
                aios_spec::governance::PolicyVerdict::Denied(reason) => {
                    action_denials.push(reason);
                    *summary.denial_counts.entry(reason).or_insert(0) += 1;
                },
            }
        }

        emitter.emit(&json!({
            "stage": "policy",
            "window_id": ctx.window_id,
            "window_ordinal": window_ordinal,
            "intent_id": intent.intent_id,
            "approved": approved_count > 0,
            "action_denials": action_denials,
            "approved_actions": approved_actions,
        }))?;
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
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort();
            let mut out = Map::new();
            for k in keys {
                if VOLATILE_KEYS.contains(&k) {
                    continue;
                }
                if let Some(v) = map.get(k) {
                    out.insert(k.to_string(), canonicalize(v));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_summary_renders_all_blocks() {
        let mut summary = ReplaySummary {
            lines_total: 3,
            events_ingested: 3,
            windows_closed: 1,
            intents_total: 3,
            intents_approved: 2,
            intents_rejected: 1,
            actions_authorized: 2,
            actions_denied: 2,
            audit_hash: "sha256:3fd30718ffd3f8953c14f4b5030b8d6ac52fd3e598ac3c9ab050554c89c3ae14"
                .into(),
            ..Default::default()
        };
        summary
            .denial_counts
            .insert(DenialReason::ActionCapabilityDenied, 2);

        let rendered = summary.human_summary();
        assert!(rendered.contains("=== DiPECS replay ==="));
        assert!(rendered.contains("sha256:3fd30718ffd3f895…"));
        assert!(rendered.contains("3 lines · 3 ingested · 0 skipped · 0 parse errors"));
        assert!(rendered.contains("windows  1 closed"));
        assert!(rendered.contains("intents  3 total · 2 approved · 1 rejected"));
        assert!(rendered.contains("actions  2 authorized · 2 denied · 0 failed · 0 audit records"));
        assert!(rendered.contains("denials by reason"));
        assert!(rendered.contains("ActionCapabilityDenied"));
        assert!(
            rendered
                .lines()
                .any(|l| l.trim() == "ActionCapabilityDenied  2"
                    || l.contains("ActionCapabilityDenied") && l.trim_end().ends_with('2')),
            "denial row must show count; got:\n{rendered}"
        );
    }

    #[test]
    fn human_summary_omits_denials_block_when_empty() {
        let summary = ReplaySummary {
            lines_total: 1,
            events_ingested: 1,
            audit_hash: "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                .into(),
            ..Default::default()
        };
        let rendered = summary.human_summary();
        assert!(
            !rendered.contains("denials by reason"),
            "empty denial_counts must not emit a section header; got:\n{rendered}"
        );
    }

    #[test]
    fn human_summary_handles_empty_audit_hash() {
        let summary = ReplaySummary::default();
        let rendered = summary.human_summary();
        assert!(
            rendered.contains("audit    (none)"),
            "empty audit hash should render '(none)' placeholder; got:\n{rendered}"
        );
    }

    #[test]
    fn human_summary_denial_table_aligns_widest_variant() {
        let mut summary = ReplaySummary::default();
        summary
            .denial_counts
            .insert(DenialReason::ConfidenceTooLow, 1);
        summary
            .denial_counts
            .insert(DenialReason::BatchActionCapExceeded, 3);
        let rendered = summary.human_summary();
        let denial_lines: Vec<&str> = rendered
            .lines()
            .skip_while(|l| !l.starts_with("denials by reason"))
            .skip(1)
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(denial_lines.len(), 2);
        let widths: Vec<usize> = denial_lines
            .iter()
            .map(|l| l.find(|c: char| c.is_ascii_digit()).unwrap_or(0))
            .collect();
        assert_eq!(
            widths[0], widths[1],
            "count column must be aligned across denial rows; got lines:\n{denial_lines:#?}"
        );
    }
}
