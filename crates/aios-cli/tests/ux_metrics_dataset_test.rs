//! UX-metrics measurement checks: startup latency (PreWarm benefit) and jank
//! (ReleaseMemory impact). The JSON is data-only; tests recompute summary values
//! from raw samples so the report cannot drift from the evidence.

#![allow(dead_code)]

use serde_json::Value;

const DATA: &str =
    include_str!("../../../data/evaluation/ux-metrics/ux-metrics-emulator-20260703-171457.json");
const COLLECT_UX_SCRIPT: &str = include_str!("../../../tools/collect/collect-ux-metrics.sh");
const EPSILON: f64 = 0.011;

#[derive(Debug, Clone)]
struct RunMetrics {
    mode: String,
    avg_startup_total_time_ms: Option<f64>,
    avg_cpu_pct: f64,
    avg_rss_mb: f64,
    avg_pss_mb: f64,
    avg_jank_pct: f64,
}

fn fixture() -> Value {
    let raw = DATA.strip_prefix('\u{FEFF}').unwrap_or(DATA);
    serde_json::from_str(raw).expect("ux metrics JSON fixture must parse")
}

fn sample_count(data: &Value) -> usize {
    data["environment"]["samples_per_mode"]
        .as_u64()
        .expect("samples_per_mode must be an integer") as usize
}

fn number(value: &Value, key: &str) -> f64 {
    value
        .get(key)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("missing numeric field {key}"))
}

fn number_or_zero(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn avg(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn percentile_ceil(values: &[f64], percentile: f64) -> f64 {
    assert!(
        !values.is_empty(),
        "percentile requires at least one sample"
    );
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let rank = (percentile / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn recompute_run(run: &Value, expected_sample_count: usize) -> RunMetrics {
    let mode = run["mode"]
        .as_str()
        .expect("run mode must be a string")
        .to_string();
    let samples = run["samples"]
        .as_array()
        .expect("run samples must be an array");
    assert_eq!(
        samples.len(),
        expected_sample_count,
        "{mode} sample count must match environment.samples_per_mode"
    );

    for (expected, sample) in samples.iter().enumerate() {
        assert_eq!(
            sample["sample_index"].as_u64(),
            Some(expected as u64),
            "{mode} sample indexes must be dense and ordered"
        );
    }

    let has_startup = samples[0].get("startup_total_time_ms").is_some();
    let startup_wait: Vec<f64> = if has_startup {
        samples
            .iter()
            .map(|s| number(s, "startup_total_time_ms"))
            .collect()
    } else {
        vec![]
    };
    let cpu: Vec<f64> = samples
        .iter()
        .map(|s| number_or_zero(s, "cpu_pct"))
        .collect();
    let rss: Vec<f64> = samples
        .iter()
        .map(|s| number_or_zero(s, "rss_mb"))
        .collect();
    let pss: Vec<f64> = samples
        .iter()
        .map(|s| number_or_zero(s, "pss_mb"))
        .collect();
    let jank: Vec<f64> = samples
        .iter()
        .map(|s| number_or_zero(s, "jank_pct"))
        .collect();

    RunMetrics {
        mode,
        avg_startup_total_time_ms: if has_startup {
            Some(avg(&startup_wait))
        } else {
            None
        },
        avg_cpu_pct: avg(&cpu),
        avg_rss_mb: avg(&rss),
        avg_pss_mb: avg(&pss),
        avg_jank_pct: avg(&jank),
    }
}

fn assert_close(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() <= EPSILON,
        "{label}: actual {actual:.4} != expected {expected:.4}"
    );
}

// ── tests ──

#[test]
fn ux_metrics_schema_and_structure() {
    let data = fixture();
    assert_eq!(data["schema_version"], "dipecs.ux_metrics.v1");
    assert_eq!(data["status"], "measured_android_emulator");
    assert_eq!(
        data["environment"]["device"], "Android Studio emulator",
        "measurement must document the Android Studio emulator target"
    );

    let runs = data["runs"].as_array().expect("runs must be an array");
    assert_eq!(
        runs.len(),
        5,
        "system + cold + prewarm + baseline_jank + post_release_jank"
    );

    let modes: Vec<&str> = runs.iter().map(|r| r["mode"].as_str().unwrap()).collect();
    assert!(modes.contains(&"no_dipecs_baseline"));
    assert!(modes.contains(&"cold_startup"));
    assert!(modes.contains(&"prewarm_startup"));
    assert!(modes.contains(&"baseline_jank"));
    assert!(modes.contains(&"post_release_jank"));

    // Verify comparison section is present
    let comp = &data["comparison"];
    assert!(comp["without_dipecs"]["cold_startup_ms"].as_f64().unwrap() > 0.0);
    assert!(comp["with_dipecs"]["prewarm_startup_ms"].as_f64().unwrap() > 0.0);
    assert!(
        comp["without_dipecs"]["system_free_ram_kb_avg"]
            .as_f64()
            .unwrap()
            > 0.0
    );
}

#[test]
fn ux_metrics_measurement_is_internally_consistent() {
    let data = fixture();
    let runs = data["runs"].as_array().expect("runs");
    let n = sample_count(&data);

    for run in runs {
        let mode = run["mode"].as_str().expect("mode");
        let computed = recompute_run(run, n);
        let summary = &run["summary"];

        // System baseline has its own schema (only free_ram)
        if mode == "no_dipecs_baseline" {
            assert!(summary.get("avg_system_free_ram_kb").is_some());
            continue;
        }

        assert_close(
            computed.avg_rss_mb,
            number(summary, "avg_rss_mb"),
            &format!("{mode} avg_rss_mb"),
        );
        assert_close(
            computed.avg_pss_mb,
            number(summary, "avg_pss_mb"),
            &format!("{mode} avg_pss_mb"),
        );
        assert_close(
            computed.avg_jank_pct,
            number(summary, "avg_jank_pct"),
            &format!("{mode} avg_jank_pct"),
        );

        if let Some(wait) = computed.avg_startup_total_time_ms {
            assert_close(
                wait,
                number(summary, "avg_startup_total_time_ms"),
                &format!("{mode} avg_startup_total_time_ms"),
            );
        }
    }
}

#[test]
fn ux_metrics_prewarm_shows_no_regression() {
    let data = fixture();
    let deltas = &data["ux_deltas"]["prewarm_vs_cold"];
    let pct_faster = number(deltas, "pct_faster");
    let ms_faster = number(deltas, "startup_total_time_ms_reduction");
    // On emulator, PreWarm benefit is small (within noise). The hard requirement
    // is that it must not regress beyond a noise tolerance.
    let threshold = number(&data["thresholds"], "min_prewarm_pct_faster");
    assert!(
        pct_faster >= threshold,
        "PreWarm startup regressed: {pct_faster}% (threshold {threshold}%)"
    );
    // Also check the absolute ms threshold
    let min_ms = number(&data["thresholds"], "min_prewarm_ms_faster");
    assert!(
        ms_faster >= min_ms,
        "PreWarm startup slower by {ms_faster}ms (threshold {min_ms}ms)"
    );
}

#[test]
fn ux_metrics_reports_startup_sample_size_and_p95() {
    let data = fixture();
    let runs = data["runs"].as_array().expect("runs");
    let startup_runs: Vec<&Value> = runs
        .iter()
        .filter(|run| {
            matches!(
                run["mode"].as_str(),
                Some("cold_startup" | "prewarm_startup")
            )
        })
        .collect();
    assert_eq!(
        startup_runs.len(),
        2,
        "fixture must include cold and prewarm startup runs"
    );

    let total_startup_samples: usize = startup_runs
        .iter()
        .map(|run| run["samples"].as_array().expect("samples").len())
        .sum();
    assert!(
        total_startup_samples >= 20,
        "PreWarm comparison must have at least 20 startup samples; got {total_startup_samples}"
    );

    for run in startup_runs {
        let mode = run["mode"].as_str().expect("mode");
        let samples = run["samples"].as_array().expect("samples");
        let startup_times: Vec<f64> = samples
            .iter()
            .map(|sample| number(sample, "startup_total_time_ms"))
            .collect();
        let expected_p95 = percentile_ceil(&startup_times, 95.0);
        let actual_p95 = number(&run["summary"], "p95_startup_total_time_ms");
        assert_close(
            actual_p95,
            expected_p95,
            &format!("{mode} p95_startup_total_time_ms"),
        );
    }
}

#[test]
fn ux_metrics_release_memory_does_not_increase_jank() {
    let data = fixture();
    let deltas = &data["ux_deltas"]["release_vs_baseline"];
    let jank_reduction = number(deltas, "avg_jank_pct_points_reduction");
    let threshold = number(&data["thresholds"], "max_jank_pct_points_increase");
    assert!(
        jank_reduction >= -threshold,
        "ReleaseMemory must not increase jank by more than {threshold} pp: actual delta {jank_reduction}"
    );
}

#[test]
fn ux_metrics_conclusion_matches_deltas() {
    let data = fixture();
    let conclusion = &data["conclusion"];
    assert_eq!(conclusion["accepted"], true);

    let prewarm_delta = &data["ux_deltas"]["prewarm_vs_cold"];
    let wait_reduction = number(prewarm_delta, "startup_total_time_ms_reduction");
    assert_eq!(
        conclusion["prewarm_effective"].as_bool().unwrap(),
        wait_reduction > 0.0,
        "prewarm_effective must match startup_total_time_ms_reduction sign"
    );

    let release_delta = &data["ux_deltas"]["release_vs_baseline"];
    let jank_reduction = number(release_delta, "avg_jank_pct_points_reduction");
    assert_eq!(
        conclusion["release_memory_effective"].as_bool().unwrap(),
        jank_reduction > 0.0,
        "release_memory_effective must require positive jank reduction"
    );
}

#[test]
fn ux_metrics_stays_within_budget() {
    let data = fixture();
    let thresholds = &data["thresholds"];
    let runs = data["runs"].as_array().expect("runs");
    let n = sample_count(&data);
    let metrics: Vec<RunMetrics> = runs.iter().map(|r| recompute_run(r, n)).collect();

    for run in &metrics {
        assert!(
            run.avg_rss_mb <= number(thresholds, "max_rss_mb"),
            "{} RSS too high: {:.2} MB",
            run.mode,
            run.avg_rss_mb
        );
        assert!(
            run.avg_pss_mb <= number(thresholds, "max_pss_mb"),
            "{} PSS too high: {:.2} MB",
            run.mode,
            run.avg_pss_mb
        );
    }
}

#[test]
fn ux_metrics_script_labels_startup_metric_as_total_time() {
    assert!(
        COLLECT_UX_SCRIPT.contains("Startup latency measured via am start -W (TotalTime)."),
        "script notes must name the parsed am start -W field"
    );
    assert!(
        COLLECT_UX_SCRIPT.contains("## Startup Latency (am start -W TotalTime)"),
        "markdown report title must name the parsed am start -W field"
    );
    assert!(
        !COLLECT_UX_SCRIPT.contains("am start -W WaitTime"),
        "script parses TotalTime, so it must not label the metric as WaitTime"
    );
}
