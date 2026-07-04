//! Resource-overhead measurement checks for the final value-data section.
//!
//! The JSON measurement is intentionally data-only: tests recompute the headline
//! averages and deltas from minute-level samples so the report cannot drift
//! away from the underlying evidence.

use serde_json::Value;

const DATA: &str = include_str!(
    "../../../data/evaluation/resource-overhead/resource-overhead-emulator-20260701-131525.json"
);
const LATEST_RESOURCE_DATA: &str = include_str!(
    "../../../data/evaluation/resource-overhead/resource-overhead-emulator-20260701-162742.json"
);
const EPSILON: f64 = 0.011;

#[derive(Debug, Clone)]
struct RunMetrics {
    mode: String,
    avg_cpu_pct: f64,
    avg_rss_mb: f64,
    avg_pss_mb: f64,
    battery_pct_delta: f64,
    thermal_delta_c: f64,
    avg_jank_pct: f64,
}

fn fixture() -> Value {
    let raw = DATA.strip_prefix('\u{FEFF}').unwrap_or(DATA);
    serde_json::from_str(raw).expect("resource overhead JSON fixture must parse")
}

fn latest_fixture() -> Value {
    let raw = LATEST_RESOURCE_DATA
        .strip_prefix('\u{FEFF}')
        .unwrap_or(LATEST_RESOURCE_DATA);
    serde_json::from_str(raw).expect("latest resource overhead JSON fixture must parse")
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

fn avg(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
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

    let cpu: Vec<f64> = samples.iter().map(|s| number(s, "cpu_pct")).collect();
    let rss: Vec<f64> = samples.iter().map(|s| number(s, "rss_mb")).collect();
    let pss: Vec<f64> = samples.iter().map(|s| number(s, "pss_mb")).collect();
    let battery: Vec<f64> = samples.iter().map(|s| number(s, "battery_pct")).collect();
    let thermal: Vec<f64> = samples.iter().map(|s| number(s, "thermal_c")).collect();
    let jank: Vec<f64> = samples.iter().map(|s| number(s, "jank_pct")).collect();

    RunMetrics {
        mode,
        avg_cpu_pct: avg(&cpu),
        avg_rss_mb: avg(&rss),
        avg_pss_mb: avg(&pss),
        battery_pct_delta: battery.first().unwrap() - battery.last().unwrap(),
        thermal_delta_c: thermal.last().unwrap() - thermal.first().unwrap(),
        avg_jank_pct: avg(&jank),
    }
}

fn assert_close(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() <= EPSILON,
        "{label}: actual {actual:.4} != expected {expected:.4}"
    );
}

#[test]
fn resource_overhead_measurement_is_internally_consistent() {
    let data = fixture();
    assert_eq!(data["schema_version"], "dipecs.resource_overhead.v1");
    assert_eq!(data["status"], "measured_android_emulator");
    assert_eq!(
        data["environment"]["device"], "Android Studio emulator",
        "measurement must document the Android Studio emulator target"
    );

    let runs = data["runs"].as_array().expect("runs must be an array");
    assert_eq!(runs.len(), 3, "baseline + observe + action-loop runs");
    let n = sample_count(&data);

    for run in runs {
        let mode = run["mode"].as_str().expect("mode");
        let computed = recompute_run(run, n);
        let summary = &run["summary"];

        assert_close(
            computed.avg_cpu_pct,
            number(summary, "avg_cpu_pct"),
            &format!("{mode} avg_cpu_pct"),
        );
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
            computed.battery_pct_delta,
            number(summary, "battery_pct_delta"),
            &format!("{mode} battery_pct_delta"),
        );
        assert_close(
            computed.thermal_delta_c,
            number(summary, "thermal_delta_c"),
            &format!("{mode} thermal_delta_c"),
        );
        assert_close(
            computed.avg_jank_pct,
            number(summary, "avg_jank_pct"),
            &format!("{mode} avg_jank_pct"),
        );
    }
}

#[test]
fn resource_overhead_fixture_stays_within_budget() {
    let data = fixture();
    let thresholds = &data["thresholds"];
    let runs = data["runs"].as_array().expect("runs");
    let n = sample_count(&data);
    let metrics: Vec<RunMetrics> = runs.iter().map(|r| recompute_run(r, n)).collect();
    let baseline = metrics
        .iter()
        .find(|m| m.mode == "baseline_idle")
        .expect("baseline run must exist");

    let mut checked_modes = 0;
    for run in metrics
        .iter()
        .filter(|m| m.mode == "dipecs_observe_only" || m.mode == "dipecs_action_loop")
    {
        checked_modes += 1;
        let cpu_delta = run.avg_cpu_pct - baseline.avg_cpu_pct;
        let rss_delta = run.avg_rss_mb - baseline.avg_rss_mb;
        let pss_delta = run.avg_pss_mb - baseline.avg_pss_mb;
        let battery_delta = run.battery_pct_delta - baseline.battery_pct_delta;
        let thermal_delta = run.thermal_delta_c;
        let jank_delta = run.avg_jank_pct - baseline.avg_jank_pct;

        assert!(
            cpu_delta <= number(thresholds, "max_cpu_delta_pct_points"),
            "{} CPU delta too high: {cpu_delta:.2}",
            run.mode
        );
        assert!(
            rss_delta <= number(thresholds, "max_rss_delta_mb"),
            "{} RSS delta too high: {rss_delta:.2}",
            run.mode
        );
        assert!(
            pss_delta <= number(thresholds, "max_pss_delta_mb"),
            "{} PSS delta too high: {pss_delta:.2}",
            run.mode
        );
        assert!(
            battery_delta <= number(thresholds, "max_battery_pct_delta"),
            "{} battery percentage delta too high: {battery_delta:.3}",
            run.mode
        );
        assert!(
            thermal_delta <= number(thresholds, "max_thermal_delta_c"),
            "{} thermal delta too high: {thermal_delta:.2}",
            run.mode
        );
        assert!(
            jank_delta <= number(thresholds, "max_jank_delta_pct_points"),
            "{} jank delta too high: {jank_delta:.2}",
            run.mode
        );
    }

    assert_eq!(checked_modes, 2, "must check observe-only and action-loop");
}

#[test]
fn resource_overhead_conclusion_matches_recomputed_deltas() {
    let data = fixture();
    assert_eq!(data["conclusion"]["accepted"], true);
    let runs = data["runs"].as_array().expect("runs");
    let n = sample_count(&data);
    let metrics: Vec<RunMetrics> = runs.iter().map(|r| recompute_run(r, n)).collect();
    let baseline = metrics
        .iter()
        .find(|m| m.mode == "baseline_idle")
        .expect("baseline run must exist");

    for run in metrics
        .iter()
        .filter(|m| m.mode == "dipecs_observe_only" || m.mode == "dipecs_action_loop")
    {
        let deltas = &data["conclusion"]["deltas_vs_baseline"][&run.mode];
        assert_close(
            run.avg_cpu_pct - baseline.avg_cpu_pct,
            number(deltas, "avg_cpu_pct_points"),
            &format!("{} cpu conclusion delta", run.mode),
        );
        assert_close(
            run.avg_rss_mb - baseline.avg_rss_mb,
            number(deltas, "avg_rss_mb"),
            &format!("{} rss conclusion delta", run.mode),
        );
        assert_close(
            run.avg_pss_mb - baseline.avg_pss_mb,
            number(deltas, "avg_pss_mb"),
            &format!("{} pss conclusion delta", run.mode),
        );
        assert_close(
            run.battery_pct_delta - baseline.battery_pct_delta,
            number(deltas, "battery_pct_delta"),
            &format!("{} battery percentage conclusion delta", run.mode),
        );
        assert_close(
            run.thermal_delta_c - baseline.thermal_delta_c,
            number(deltas, "thermal_delta_c"),
            &format!("{} thermal conclusion delta", run.mode),
        );
        assert_close(
            run.avg_jank_pct - baseline.avg_jank_pct,
            number(deltas, "avg_jank_pct_points"),
            &format!("{} jank conclusion delta", run.mode),
        );
    }
}

#[test]
fn simulated_power_thermal_estimates_are_labeled_and_bounded() {
    let data = fixture();
    let estimated = &data["estimated_power_thermal"];
    assert_eq!(
        estimated["status"], "simulated_from_measured_cpu_pss",
        "estimated battery/thermal values must be labeled as simulated"
    );

    let thresholds = &data["thresholds"];
    for mode in ["dipecs_observe_only", "dipecs_action_loop"] {
        let estimate = &estimated["estimates_vs_baseline"][mode];
        let mah_per_min = number(estimate, "estimated_battery_mah_per_min");
        let thermal_delta = number(estimate, "estimated_thermal_delta_c");

        assert!(
            mah_per_min > 0.0,
            "{mode} estimated battery drain should be positive"
        );
        assert!(
            mah_per_min <= number(thresholds, "max_estimated_battery_mah_per_min"),
            "{mode} estimated battery drain too high: {mah_per_min:.3}"
        );
        assert!(
            thermal_delta > 0.0,
            "{mode} estimated thermal delta should be positive"
        );
        assert!(
            thermal_delta <= number(thresholds, "max_estimated_thermal_delta_c"),
            "{mode} estimated thermal delta too high: {thermal_delta:.2}"
        );
    }
}

#[test]
fn report_summary_merges_measured_and_estimated_values() {
    let data = fixture();
    let report = &data["report_summary"];
    assert_eq!(report["status"], "measured_with_estimated_power_thermal");

    let rows = report["rows"].as_array().expect("report rows");
    assert_eq!(rows.len(), 3);

    // Cross-validate: report row values must match the corresponding run summary.
    let n = sample_count(&data);
    let runs = data["runs"].as_array().expect("runs");
    let metrics: Vec<RunMetrics> = runs.iter().map(|r| recompute_run(r, n)).collect();

    for run in &metrics {
        let row = rows
            .iter()
            .find(|r| r["mode"] == run.mode.as_str())
            .expect("report row must exist for each run mode");

        assert_close(
            number(row, "avg_cpu_pct"),
            run.avg_cpu_pct,
            &format!("report row {} avg_cpu_pct", run.mode),
        );
        assert_close(
            number(row, "avg_rss_mb"),
            run.avg_rss_mb,
            &format!("report row {} avg_rss_mb", run.mode),
        );
        assert_close(
            number(row, "avg_pss_mb"),
            run.avg_pss_mb,
            &format!("report row {} avg_pss_mb", run.mode),
        );
        assert_close(
            number(row, "avg_jank_pct"),
            run.avg_jank_pct,
            &format!("report row {} avg_jank_pct", run.mode),
        );
    }

    // Action-loop estimates must be positive (not zero), since PSS delta > 0.
    let action_loop = rows
        .iter()
        .find(|row| row["mode"] == "dipecs_action_loop")
        .expect("action-loop report row");
    assert!(
        number(action_loop, "estimated_battery_mah_per_min") > 0.0,
        "report should not present action-loop battery as zero"
    );
    assert!(
        number(action_loop, "estimated_thermal_delta_c") > 0.0,
        "report should not present action-loop thermal as zero"
    );
}

#[test]
fn latest_resource_overhead_marks_cpu_as_noisy_budget_smoke() {
    let data = latest_fixture();
    let notes = data["notes"]
        .as_array()
        .expect("notes must be an array")
        .iter()
        .map(|note| note.as_str().expect("note must be a string"))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        notes.contains("below measurement precision"),
        "latest resource-overhead fixture must not present 0.0% CPU as exact"
    );

    let report_note = data["report_summary"]["note"]
        .as_str()
        .expect("report summary note must be a string");
    assert!(
        report_note.contains("noisy budget smoke"),
        "report summary must describe CPU as a noisy budget smoke"
    );

    let action_loop = data["report_summary"]["rows"]
        .as_array()
        .expect("report rows")
        .iter()
        .find(|row| row["mode"] == "dipecs_action_loop")
        .expect("action-loop row");
    assert_eq!(
        number(action_loop, "avg_cpu_pct"),
        0.0,
        "this regression fixture covers the historical 0.0% CPU reading"
    );
}
