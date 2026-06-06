//! Summary statistics for Lab4 JSONL result files.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::command::BenchRecord;
use crate::error::Lab4Result;
use crate::jsonl;

/// Aggregated benchmark summary for one mode.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ModeSummary {
    /// Experiment mode label.
    pub mode: String,
    /// Total record count.
    pub records: usize,
    /// Number of records with successful exit status.
    pub successes: usize,
    /// Average wall-clock duration in milliseconds.
    pub average_duration_ms: f64,
    /// Average throughput across records that expose tokens/s.
    pub average_tokens_per_second: Option<f64>,
}

/// Loads benchmark records from JSONL and groups them by mode.
///
/// # Errors
///
/// Returns an error if the JSONL file cannot be read or decoded as JSON
/// values.
pub fn summarize_bench_file(path: &Path) -> Lab4Result<Vec<ModeSummary>> {
    let records: Vec<Value> = jsonl::read_jsonl(path)?;
    Ok(summarize_json_records(&records))
}

/// Groups benchmark records by mode and computes average metrics.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn summarize_bench_records(records: &[BenchRecord]) -> Vec<ModeSummary> {
    let mut grouped: BTreeMap<String, Vec<&BenchRecord>> = BTreeMap::new();
    for record in records {
        grouped.entry(record.mode.clone()).or_default().push(record);
    }

    grouped
        .into_iter()
        .map(|(mode, records)| {
            let duration_sum: u128 = records.iter().map(|record| record.duration_ms).sum();
            let successes = records
                .iter()
                .filter(|record| record.exit_code == Some(0))
                .count();
            let token_rates: Vec<f64> = records
                .iter()
                .filter_map(|record| record.tokens_per_second)
                .collect();
            let average_tokens_per_second = average(&token_rates);
            ModeSummary {
                mode,
                records: records.len(),
                successes,
                average_duration_ms: duration_sum as f64 / records.len() as f64,
                average_tokens_per_second,
            }
        })
        .collect()
}

/// Groups generic benchmark JSON records by mode.
///
/// This accepts both `lab4-bench` records with `duration_ms` and
/// `lab4-llama bench` records with `total_duration_ms`.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn summarize_json_records(records: &[Value]) -> Vec<ModeSummary> {
    let mut grouped: BTreeMap<String, Vec<JsonMetric>> = BTreeMap::new();
    for record in records {
        if let Some(metric) = JsonMetric::from_value(record) {
            grouped.entry(metric.mode.clone()).or_default().push(metric);
        }
    }

    grouped
        .into_iter()
        .filter(|(_, records)| !records.is_empty())
        .map(|(mode, records)| {
            let duration_sum: u128 = records.iter().map(|record| record.duration_ms).sum();
            let successes = records.iter().filter(|record| record.success).count();
            let token_rates: Vec<f64> = records
                .iter()
                .filter_map(|record| record.tokens_per_second)
                .collect();
            ModeSummary {
                mode,
                records: records.len(),
                successes,
                average_duration_ms: duration_sum as f64 / records.len() as f64,
                average_tokens_per_second: average(&token_rates),
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
struct JsonMetric {
    mode: String,
    duration_ms: u128,
    success: bool,
    tokens_per_second: Option<f64>,
}

impl JsonMetric {
    fn from_value(value: &Value) -> Option<Self> {
        let mode = value
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let duration_ms = value
            .get("duration_ms")
            .or_else(|| value.get("total_duration_ms"))
            .and_then(Value::as_u64)
            .map(u128::from)?;
        let success = value
            .get("exit_code")
            .and_then(Value::as_i64)
            .is_none_or(|exit_code| exit_code == 0);
        let tokens_per_second = value.get("tokens_per_second").and_then(Value::as_f64);
        Some(Self {
            mode,
            duration_ms,
            success,
            tokens_per_second,
        })
    }
}

#[allow(clippy::cast_precision_loss)]
fn average(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn test_summarize_bench_records__groups_by_mode() {
        let records = vec![
            record("single", 100, Some(10.0), Some(0)),
            record("single", 300, Some(30.0), Some(0)),
            record("rpc", 200, None, Some(1)),
        ];

        let summaries = summarize_bench_records(&records);
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].mode, "rpc");
        assert_eq!(summaries[0].successes, 0);
        assert_eq!(summaries[1].mode, "single");
        assert!((summaries[1].average_duration_ms - 200.0).abs() < f64::EPSILON);
        assert_eq!(summaries[1].average_tokens_per_second, Some(20.0));
    }

    #[test]
    fn test_summarize_json_records__accepts_llama_records() {
        let records = vec![
            serde_json::json!({
                "mode": "single",
                "total_duration_ms": 10,
                "tokens_per_second": 2.0
            }),
            serde_json::json!({
                "mode": "single",
                "total_duration_ms": 30,
                "tokens_per_second": 6.0
            }),
        ];

        let summaries = summarize_json_records(&records);

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].records, 2);
        assert_eq!(summaries[0].successes, 2);
        assert!((summaries[0].average_duration_ms - 20.0).abs() < f64::EPSILON);
        assert_eq!(summaries[0].average_tokens_per_second, Some(4.0));
    }

    fn record(
        mode: &str,
        duration_ms: u128,
        tokens_per_second: Option<f64>,
        exit_code: Option<i32>,
    ) -> BenchRecord {
        BenchRecord {
            case_id: format!("{mode}-{duration_ms}"),
            prompt_id: "p".to_owned(),
            category: "os".to_owned(),
            mode: mode.to_owned(),
            started_at_unix_ms: 0,
            duration_ms,
            exit_code,
            tokens_per_second,
            stdout_bytes: 0,
            stderr_bytes: 0,
            stdout: String::new(),
            stderr: String::new(),
            command: Vec::new(),
        }
    }
}
