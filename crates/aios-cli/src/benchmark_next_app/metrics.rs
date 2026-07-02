//! Metric aggregation for the next-app benchmark.

use super::types::{BackendMetrics, LatencySummary};

#[derive(Debug, Clone, Default)]
pub struct PredictionRecord {
    pub rank: Option<usize>,
    pub latency_us: u64,
    pub noop: bool,
    /// True when the underlying predictor produced at least one intent with
    /// non-empty `rationale_tags`. False for statistical baselines.
    pub rationale_present: bool,
}

pub fn compute_backend_metrics(records: &[PredictionRecord], eligible: usize) -> BackendMetrics {
    let predicted = records.iter().filter(|r| r.rank.is_some()).count();
    let top1 = records.iter().filter(|r| r.rank == Some(1)).count();
    let top3 = records
        .iter()
        .filter(|r| r.rank.is_some_and(|rank| rank <= 3))
        .count();
    let rr_sum: f64 = records
        .iter()
        .map(|r| r.rank.map_or(0.0, |rank| 1.0 / rank as f64))
        .sum();
    let noop = records.iter().filter(|r| r.noop).count();
    let rationale_windows = records.iter().filter(|r| r.rationale_present).count();

    let latencies: Vec<u64> = records.iter().map(|r| r.latency_us).collect();

    BackendMetrics {
        eligible_windows: eligible,
        predicted_windows: predicted,
        top1_hits: top1,
        top3_hits: top3,
        top1_accuracy_pct: pct(top1, eligible),
        top3_accuracy_pct: pct(top3, eligible),
        prediction_coverage_pct: pct(predicted, eligible),
        conditional_top1_accuracy_pct: pct(top1, predicted),
        wrong_prediction_rate_pct: pct(predicted.saturating_sub(top1), predicted),
        no_prediction_rate_pct: pct(eligible.saturating_sub(predicted), eligible),
        noop_windows: noop,
        noop_rate_pct: pct(noop, records.len()),
        mean_reciprocal_rank: round3(rr_sum / eligible.max(1) as f64),
        macro_top1_accuracy_pct: None,
        macro_top3_accuracy_pct: None,
        rationale_coverage_pct: pct(rationale_windows, records.len()),
        latency_us: latency_summary(&latencies),
    }
}

pub fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn pct(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        round3((part as f64 / whole as f64) * 100.0)
    }
}

pub fn latency_summary(latencies: &[u64]) -> LatencySummary {
    if latencies.is_empty() {
        return LatencySummary::default();
    }
    let mut sorted = latencies.to_vec();
    sorted.sort_unstable();
    let mean = sorted.iter().map(|&x| x as f64).sum::<f64>() / sorted.len() as f64;
    LatencySummary {
        mean: round3(mean),
        p50: percentile(&sorted, 0.5),
        p95: percentile(&sorted, 0.95),
    }
}

fn percentile(sorted: &[u64], p: f64) -> f64 {
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)] as f64
}
