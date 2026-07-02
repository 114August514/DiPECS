//! Shared, lazily-initialized `BenchmarkReport` cache for baseline tests.
//!
//! Multiple integration baselines run the same next-app benchmark with the
//! default configuration. Centralizing the report behind `OnceLock` avoids
//! repeating the expensive benchmark run in every test.

use std::sync::OnceLock;

use aios_cli::benchmark_next_app::runner::{run_benchmark, BenchmarkRunConfig};
use aios_cli::benchmark_next_app::types::BenchmarkReport;

use crate::helpers::repo_root;

fn default_config() -> BenchmarkRunConfig {
    BenchmarkRunConfig {
        inputs: vec![
            repo_root().join("data/traces/scenarios/morning-routine.jsonl"),
            repo_root().join("data/traces/scenarios/multi-app-switching.jsonl"),
            repo_root().join("data/traces/scenarios/rich-workflow.jsonl"),
        ],
        labels: repo_root().join("data/traces/synthetic-next-app-v1.labels.jsonl"),
        train_split: 0.7,
        window_secs: 10,
    }
}

pub fn cached_report() -> &'static BenchmarkReport {
    static REPORT: OnceLock<BenchmarkReport> = OnceLock::new();
    REPORT.get_or_init(|| run_benchmark(&default_config()).expect("benchmark should run"))
}
