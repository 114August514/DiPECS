//! Load labels and build per-label `StructuredContext` windows from raw traces.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use aios_core::collector_ingress::RustCollectorIngress;
use aios_core::context_builder::WindowAggregator;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_spec::traits::PrivacySanitizer;
use aios_spec::{CollectorEnvelope, RawEvent, SanitizedEvent, SourceTier, StructuredContext};
use anyhow::{Context, Result};
use serde_json::Value;

use super::types::NextAppLabel;

/// Load labels from a JSONL file.
pub fn load_labels(path: &Path) -> Result<Vec<NextAppLabel>> {
    let file = File::open(path).with_context(|| format!("opening labels {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut labels = Vec::new();
    for (line_no, line) in reader.lines().enumerate() {
        let line =
            line.with_context(|| format!("reading line {} of {}", line_no + 1, path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        let label: NextAppLabel = serde_json::from_str(&line)
            .with_context(|| format!("parsing label line {} of {}", line_no + 1, path.display()))?;
        labels.push(label);
    }
    Ok(labels)
}

/// Build the ordered list of observable candidates from a context, excluding the current app.
pub fn extract_observable_candidates(ctx: &StructuredContext, current_app: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for pkg in ctx
        .summary
        .foreground_apps
        .iter()
        .chain(ctx.summary.notified_apps.iter())
    {
        if pkg == current_app || !seen.insert(pkg.clone()) {
            continue;
        }
        out.push(pkg.clone());
    }
    out
}

/// Build a map from (scenario, window_start_ms, window_end_ms) to a `StructuredContext`.
///
/// For each input trace, we parse its events, sanitize them, then for every label of that
/// scenario we create an independent `WindowAggregator` covering exactly the label's time
/// range. This matches the sliding-window labels even though `WindowAggregator` itself is
/// tumbling-window by design.
pub fn load_contexts_by_label(
    inputs: &[(String, &Path)],
    labels: &[NextAppLabel],
    window_secs: u64,
) -> Result<BTreeMap<(String, i64, i64), StructuredContext>> {
    let mut contexts = BTreeMap::new();

    for (scenario_name, trace_path) in inputs {
        let sanitized = load_and_sanitize_trace(trace_path)
            .with_context(|| format!("loading trace {}", trace_path.display()))?;

        for label in labels.iter().filter(|l| &l.scenario == scenario_name) {
            let ctx = build_context_for_label(
                &sanitized,
                window_secs,
                label.window_start_ms,
                label.window_end_ms,
            )
            .with_context(|| {
                format!(
                    "building context for scenario {} window {}-{}",
                    scenario_name, label.window_start_ms, label.window_end_ms
                )
            })?;
            contexts.insert(
                (
                    scenario_name.clone(),
                    label.window_start_ms,
                    label.window_end_ms,
                ),
                ctx,
            );
        }
    }

    Ok(contexts)
}

fn load_and_sanitize_trace(path: &Path) -> Result<Vec<SanitizedEvent>> {
    let file = File::open(path).with_context(|| format!("opening trace {}", path.display()))?;
    let reader = BufReader::new(file);
    let ingress = RustCollectorIngress;
    let sanitizer = DefaultPrivacyAirGap;
    let mut events = Vec::new();

    for (line_no, line) in reader.lines().enumerate() {
        let line =
            line.with_context(|| format!("reading line {} of {}", line_no + 1, path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)
            .with_context(|| format!("parsing JSON line {} of {}", line_no + 1, path.display()))?;

        let raw_event_value = value.get("rawEvent").cloned().unwrap_or(Value::Null);
        if raw_event_value.is_null() {
            continue;
        }
        let raw_event: RawEvent = serde_json::from_value(raw_event_value).with_context(|| {
            format!(
                "parsing rawEvent line {} of {}",
                line_no + 1,
                path.display()
            )
        })?;

        let envelope = CollectorEnvelope {
            schema_version: "dipecs.collector.v1".to_string(),
            source: value
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("apps.android-collector.replay")
                .to_string(),
            source_tier: SourceTier::PublicApi,
            device_trace_id: value
                .get("eventId")
                .and_then(Value::as_str)
                .map(String::from),
            captured_at_ms: value
                .get("timestampMs")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            received_at_ms: None,
            raw_event,
        };

        let ingested = ingress
            .accept(envelope)
            .with_context(|| format!("ingress line {} of {}", line_no + 1, path.display()))?;
        let sanitized = sanitizer.sanitize_with_tier(ingested.raw_event, ingested.source_tier);
        events.push(sanitized);
    }

    Ok(events)
}

fn build_context_for_label(
    sanitized_events: &[SanitizedEvent],
    window_secs: u64,
    window_start_ms: i64,
    window_end_ms: i64,
) -> Result<StructuredContext> {
    let mut aggregator = WindowAggregator::new(window_secs, window_start_ms);
    for event in sanitized_events {
        if event.timestamp_ms >= window_start_ms && event.timestamp_ms < window_end_ms {
            aggregator.push(event.clone());
        }
    }
    aggregator
        .close(window_end_ms)
        .context("no events in window")
}
