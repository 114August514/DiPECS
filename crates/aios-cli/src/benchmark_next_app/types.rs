//! Types for the next-app prediction benchmark.

use std::path::PathBuf;

use aios_spec::StructuredContext;
use serde::{Deserialize, Serialize};

/// One row of `synthetic-next-app-v1.labels.jsonl`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NextAppLabel {
    pub dataset_id: String,
    pub scenario: String,
    pub window_start_ms: i64,
    pub window_end_ms: i64,
    pub prediction_horizon_ms: i64,
    pub current_app: String,
    pub observable_candidates: Vec<String>,
    pub actual_next_app: Option<String>,
    pub eligible: bool,
    pub excluded_reason: Option<String>,
}

/// A single scored app candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoredPrediction {
    pub package: String,
    pub score: f32,
}

/// Result of a next-app predictor for one window.
#[derive(Debug, Clone)]
pub struct PredictionResult {
    pub ranked: Vec<ScoredPrediction>,
    pub latency_us: u64,
    /// True when at least one intent produced by this predictor carried a non-empty
    /// `rationale_tags` vec. Always `false` for statistical baselines that do not
    /// produce DiPECS intents.
    pub rationale_present: bool,
}

/// Trait implemented by every backend/baseline in the benchmark.
pub trait NextAppPredictor {
    fn name(&self) -> &'static str;

    fn train(&mut self, _train: &[NextAppLabel]) {}

    fn predict(
        &self,
        ctx: &StructuredContext,
        current_app: &str,
        candidates: &[String],
    ) -> PredictionResult;
}

/// Benchmark configuration, persisted in the report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkConfig {
    pub window_secs: u64,
    pub horizon_secs: i64,
    pub label_definition: String,
    pub eligibility_definition: String,
    pub train_split: f64,
}

/// Exclusion reason counts.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ExclusionCounts {
    pub eligible: usize,
    pub label_not_observable: usize,
    pub no_future_switch: usize,
}

/// Latency summary in microseconds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LatencySummary {
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
}

/// Metrics for a single backend in a single aggregate or scenario block.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BackendMetrics {
    pub eligible_windows: usize,
    pub predicted_windows: usize,
    pub top1_hits: usize,
    pub top3_hits: usize,
    pub top1_accuracy_pct: f64,
    pub top3_accuracy_pct: f64,
    pub prediction_coverage_pct: f64,
    pub conditional_top1_accuracy_pct: f64,
    pub wrong_prediction_rate_pct: f64,
    pub no_prediction_rate_pct: f64,
    pub noop_windows: usize,
    pub noop_rate_pct: f64,
    pub mean_reciprocal_rank: f64,
    pub macro_top1_accuracy_pct: Option<f64>,
    pub macro_top3_accuracy_pct: Option<f64>,
    /// Percentage of windows where at least one produced intent had non-empty
    /// `rationale_tags`. Meaningful only for DiPECS backends (`rule_based`,
    /// `local_evaluator`, `cloud_llm`); statistical baselines always report 0.0.
    pub rationale_coverage_pct: f64,
    pub latency_us: LatencySummary,
}

/// Per-scenario report block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScenarioReport {
    pub scenario: String,
    pub windows: usize,
    pub future_switch_windows: usize,
    pub eligible_windows: usize,
    pub train_windows: usize,
    pub test_windows: usize,
    pub exclusions: ExclusionCounts,
    pub backends: std::collections::BTreeMap<String, BackendMetrics>,
}

/// Top-level benchmark report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkReport {
    pub schema_version: String,
    pub dataset_id: String,
    pub source: String,
    pub config: BenchmarkConfig,
    pub inputs: Vec<PathBuf>,
    pub total_windows: usize,
    pub eligible_windows: usize,
    pub train_windows: usize,
    pub test_windows: usize,
    pub context_supported_switch_coverage_pct: f64,
    pub exclusions: ExclusionCounts,
    pub aggregate: std::collections::BTreeMap<String, BackendMetrics>,
    pub scenarios: Vec<ScenarioReport>,
    pub limitations: Vec<String>,
}
