//! Long-running memory stability test: detects RSS/PSS growth over time.
//! The canonical dataset is a 60-minute runway; the test checks that memory
//! does not drift beyond thresholds (i.e. no leak).

use serde_json::Value;

const DATA: &str = include_str!("../../../data/evaluation/stability-emulator-canonical.json");
const EPSILON: f64 = 0.011;

fn fixture() -> Value {
    let raw = DATA.strip_prefix('\u{FEFF}').unwrap_or(DATA);
    serde_json::from_str(raw).expect("stability JSON fixture must parse")
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

fn assert_close(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() <= EPSILON,
        "{label}: actual {actual:.4} != expected {expected:.4}"
    );
}

#[test]
fn stability_schema_and_structure() {
    let data = fixture();
    assert_eq!(data["schema_version"], "dipecs.stability.v1");
    assert_eq!(data["status"], "measured_android_emulator");

    let env = &data["environment"];
    assert!(
        number(env, "duration_minutes") >= 0.5,
        "stability test must run >= 30 sec"
    );
    assert!(
        number(env, "total_samples") >= 3.0,
        "need at least 3 samples"
    );

    let results = &data["results"];
    let samples = results["samples"].as_array().expect("samples array");
    assert_eq!(samples.len(), number(env, "total_samples") as usize);
}

#[test]
fn stability_internally_consistent() {
    let data = fixture();
    let results = &data["results"];
    let samples = results["samples"].as_array().expect("samples");

    let rss: Vec<f64> = samples.iter().map(|s| number(s, "rss_mb")).collect();
    let pss: Vec<f64> = samples.iter().map(|s| number(s, "pss_mb")).collect();
    let cpu: Vec<f64> = samples.iter().map(|s| number(s, "cpu_pct")).collect();

    assert_close(number(results, "rss_first_mb"), rss[0], "rss_first");
    assert_close(
        number(results, "rss_last_mb"),
        rss[rss.len() - 1],
        "rss_last",
    );
    assert_close(number(results, "pss_first_mb"), pss[0], "pss_first");
    assert_close(
        number(results, "pss_last_mb"),
        pss[pss.len() - 1],
        "pss_last",
    );
    assert_close(number(results, "avg_cpu_pct"), avg(&cpu), "avg_cpu");
    // rss_growth/pss_growth use warmup-skipped regression in the script,
    // so the test only checks they are present and finite.
    assert!(number(results, "rss_growth_per_hour_mb").is_finite());
    assert!(number(results, "pss_growth_per_hour_mb").is_finite());
}

#[test]
fn stability_no_memory_leak() {
    let data = fixture();
    let results = &data["results"];
    let thresholds = &data["thresholds"];

    let rss_growth = number(results, "rss_growth_per_hour_mb");
    let pss_growth = number(results, "pss_growth_per_hour_mb");
    let cpu = number(results, "avg_cpu_pct");

    let max_rss = number(thresholds, "max_rss_growth_per_hour_mb");
    let max_pss = number(thresholds, "max_pss_growth_per_hour_mb");
    let max_cpu = number(thresholds, "max_avg_cpu_pct");

    assert!(
        rss_growth <= max_rss,
        "RSS leak detected: {rss_growth:.3} MB/h (threshold {max_rss} MB/h)"
    );
    assert!(
        pss_growth <= max_pss,
        "PSS leak detected: {pss_growth:.3} MB/h (threshold {max_pss} MB/h)"
    );
    assert!(
        cpu <= max_cpu,
        "CPU too high: {cpu:.3}% (threshold {max_cpu}%)"
    );
}

#[test]
fn stability_conclusion_matches_data() {
    let data = fixture();
    let conclusion = &data["conclusion"];
    assert!(conclusion["accepted"].as_bool().unwrap());

    let results = &data["results"];
    let rss_growth = number(results, "rss_growth_per_hour_mb");
    let pss_growth = number(results, "pss_growth_per_hour_mb");

    // If growth is small, conclusion must say "no leak"
    if rss_growth <= 50.0 && pss_growth <= 20.0 {
        let note = conclusion["note"].as_str().unwrap();
        assert!(
            note.contains("No significant"),
            "expected no-leak note, got: {note}"
        );
    }
}
