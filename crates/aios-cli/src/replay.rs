//! JSONL replay — drive the core pipeline from an Android `CollectorEvent` trace.
//!
//! Each input line is the Android `CollectorEvent` JSON shape; we extract its
//! inner `rawEvent`, synthesize a `CollectorEnvelope`, and push it through
//! `RustCollectorIngress → DefaultPrivacyAirGap → WindowAggregator →
//! DecisionRouter → PolicyEngine`. Window boundaries use the captured
//! timestamps from the trace, not wall-clock time — replay is deterministic.

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
use serde_json::{json, Value};

const SCHEMA_VERSION: &str = "dipecs.collector.v1";

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
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct ReplaySummary {
    pub lines_total: u64,
    pub lines_skipped_no_raw_event: u64,
    pub lines_parse_error: u64,
    pub events_ingested: u64,
    pub windows_closed: u64,
    pub intents_total: u64,
    pub actions_authorized: u64,
}

/// Replay a JSONL stream through the core pipeline.
///
/// `window_secs` controls the `WindowAggregator` window. `stage` gates the
/// highest pipeline phase that is exercised *and* emitted.
pub fn run<R: BufRead, W: Write>(
    reader: R,
    writer: &mut W,
    window_secs: u64,
    stage: Stage,
) -> Result<ReplaySummary> {
    let ingress = RustCollectorIngress;
    let sanitizer = DefaultPrivacyAirGap;
    let router = DecisionRouter::default();
    let policy = PolicyEngine::default();
    let executor = DefaultActionExecutor;
    let mut summary = ReplaySummary::default();
    let mut aggregator: Option<WindowAggregator> = None;
    let mut last_captured_at_ms: i64 = 0;

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
                emit(
                    writer,
                    &json!({
                        "stage": "error",
                        "line": line_no,
                        "error": format!("invalid JSON: {e}"),
                    }),
                )?;
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
                emit(
                    writer,
                    &json!({
                        "stage": "error",
                        "line": line_no,
                        "error": format!("rawEvent does not match RawEvent shape: {e}"),
                    }),
                )?;
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
            emit(
                writer,
                &json!({
                    "stage": "ingest",
                    "line": line_no,
                    "source_tier": format!("{:?}", ingested.source_tier),
                    "raw_event_kind": raw_event_kind(&ingested.raw_event),
                }),
            )?;
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
                    writer,
                )?;
            }
        }

        if !stage.includes(Stage::Sanitize) {
            continue;
        }
        let sanitized = sanitizer.sanitize_with_tier(ingested.raw_event, ingested.source_tier);
        emit(
            writer,
            &json!({
                "stage": "sanitize",
                "line": line_no,
                "sanitized": sanitized,
            }),
        )?;
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
                writer,
            )?;
        }
    }

    emit(
        writer,
        &json!({
            "stage": "summary",
            "summary": summary,
        }),
    )?;

    Ok(summary)
}

fn process_window<W: Write>(
    ctx: &StructuredContext,
    router: &DecisionRouter,
    policy: &PolicyEngine,
    executor: &DefaultActionExecutor,
    stage: Stage,
    summary: &mut ReplaySummary,
    writer: &mut W,
) -> Result<()> {
    summary.windows_closed += 1;

    if stage.includes(Stage::Context) {
        emit(
            writer,
            &json!({
                "stage": "context",
                "window_id": ctx.window_id,
                "window_start_ms": ctx.window_start_ms,
                "window_end_ms": ctx.window_end_ms,
                "duration_secs": ctx.duration_secs,
                "event_count": ctx.events.len(),
                "summary": ctx.summary,
            }),
        )?;
    }

    if !stage.includes(Stage::Decision) {
        return Ok(());
    }
    let decision = router.evaluate(ctx);
    summary.intents_total += decision.intent_batch.intents.len() as u64;
    emit(
        writer,
        &json!({
            "stage": "decision",
            "window_id": ctx.window_id,
            "route": format!("{:?}", decision.route),
            "model": decision.intent_batch.model,
            "intent_count": decision.intent_batch.intents.len(),
            "rationale_tags": decision.rationale_tags,
            "error": decision.error,
        }),
    )?;

    if !stage.includes(Stage::Policy) {
        return Ok(());
    }
    let capability = CapabilityLevel::for_route(decision.route);
    let decisions = policy.evaluate_batch_with_capability(&decision.intent_batch, &capability);
    for d in &decisions {
        if d.approved {
            summary.actions_authorized += d.approved_actions.len() as u64;
        }
        emit(
            writer,
            &json!({
                "stage": "policy",
                "window_id": ctx.window_id,
                "intent_id": d.intent_id,
                "approved": d.approved,
                "rejection_reason": d.rejection_reason,
                "capability_denials": d.capability_denials,
                "approved_actions": d.approved_actions,
            }),
        )?;
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
            emit(
                writer,
                &json!({
                    "stage": "execute",
                    "window_id": ctx.window_id,
                    "intent_id": d.intent_id,
                    "action_type": r.action_type,
                    "success": r.success,
                    "error": r.error,
                }),
            )?;
        }
    }
    Ok(())
}

fn emit<W: Write>(writer: &mut W, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
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
