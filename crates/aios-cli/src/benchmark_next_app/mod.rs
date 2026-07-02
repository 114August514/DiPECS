//! Next-app prediction benchmark for DiPECS.
//!
//! Provides a reproducible backtest harness that evaluates DiPECS decision
//! backends and simple baselines against ground-truth labels derived from
//! synthetic Android traces.

pub mod baselines;
pub mod context_loader;
pub mod metrics;
pub mod predictor;
pub mod report;
pub mod runner;
pub mod types;

pub use runner::{run_benchmark, BenchmarkRunConfig};
pub use types::{
    BackendMetrics, BenchmarkConfig, BenchmarkReport, ExclusionCounts, LatencySummary,
    NextAppLabel, NextAppPredictor, PredictionResult, ScoredPrediction,
};
