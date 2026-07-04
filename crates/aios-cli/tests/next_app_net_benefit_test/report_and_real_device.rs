//! Integration test: DiPECS ensemble next-app net benefit vs. strong predictive baseline.
//!
//! The test uses committed evaluation fixtures:
//! - `data/evaluation/next-app/lsapp-standard.report.json` for hit rates and example count.
//! - `data/evaluation/next-app/prewarm-net-benefit-real-device-20260704-184148.json`
//!   for n>=20 Pixel 6a PreWarm hit/miss startup measurements.

use aios_cli::next_app::{compute_net_benefit, NetBenefitInputs};

use super::support::{find_net_benefit_report, find_report, find_ux_metrics, load_json};

#[test]
fn lsapp_report_contains_strong_predictive_baseline() {
    let report_path = find_report().expect("lsapp-standard.report.json fixture must be committed");
    let report = load_json(&report_path).expect("lsapp-standard.report.json must parse");
    assert_eq!(
        report.get("split").and_then(|v| v.as_str()),
        Some("Standard"),
        "lsapp-standard.report.json must be the Standard split"
    );

    let test_examples = report
        .get("test_examples")
        .and_then(|v| v.as_u64())
        .expect("test_examples missing from report");
    let strong = report
        .get("metrics")
        .and_then(|m| m.get("strong_predictive"))
        .unwrap_or_else(|| {
            panic!(
                "strong_predictive metrics missing from {}; regenerate the report",
                report_path.display()
            )
        });
    assert_eq!(
        strong.get("examples").and_then(|v| v.as_u64()),
        Some(test_examples),
        "strong_predictive must evaluate the same test window count"
    );
    assert!(
        strong
            .get("hit_rate_at_1_pct")
            .and_then(|v| v.as_f64())
            .unwrap_or_default()
            > 0.0,
        "strong_predictive must produce a non-zero hit@1 baseline"
    );
}

#[test]
fn lsapp_standard_report_ensemble_beats_strong_predictive_top_k() {
    let report_path = find_report().expect("lsapp-standard.report.json fixture must be committed");
    let report = load_json(&report_path).expect("lsapp-standard.report.json must parse");
    let metrics = report.get("metrics").expect("metrics missing from report");
    let ensemble = metrics
        .get("ensemble")
        .expect("ensemble metrics missing from report");
    let strong = metrics
        .get("strong_predictive")
        .expect("strong_predictive metrics missing from report");
    let test_examples = report
        .get("test_examples")
        .and_then(|v| v.as_u64())
        .expect("test_examples missing from report");

    assert_eq!(
        ensemble.get("examples").and_then(|v| v.as_u64()),
        Some(test_examples),
        "ensemble must evaluate the same Standard test window count"
    );
    assert_eq!(
        strong.get("examples").and_then(|v| v.as_u64()),
        Some(test_examples),
        "strong_predictive must evaluate the same Standard test window count"
    );

    for field in [
        "hit_rate_at_1_pct",
        "hit_rate_at_3_pct",
        "hit_rate_at_5_pct",
    ] {
        let ensemble_hit = ensemble
            .get(field)
            .and_then(|v| v.as_f64())
            .unwrap_or_else(|| panic!("ensemble {field} missing from report"));
        let strong_hit = strong
            .get(field)
            .and_then(|v| v.as_f64())
            .unwrap_or_else(|| panic!("strong_predictive {field} missing from report"));
        assert!(
            ensemble_hit > strong_hit,
            "Standard ensemble {field} ({ensemble_hit:.3}%) must beat strong_predictive ({strong_hit:.3}%)"
        );
    }
}

#[test]
fn ux_metrics_fixture_resolves_total_time_measurement_run() {
    let ux_path = find_ux_metrics().expect("ux-metrics fixture must be committed");
    let ux = load_json(&ux_path).expect("ux-metrics fixture must parse");

    assert!(
        ux.get("notes")
            .and_then(|v| v.as_array())
            .expect("notes must be present")
            .iter()
            .any(|note| note.as_str().unwrap_or_default().contains("TotalTime")),
        "{} must be the newer TotalTime-based UX fixture",
        ux_path.display()
    );
    let runs = ux
        .get("runs")
        .and_then(|v| v.as_array())
        .expect("runs must be present");
    let mut startup_samples = 0usize;
    for mode in ["cold_startup", "prewarm_startup"] {
        let run = runs
            .iter()
            .find(|run| run.get("mode").and_then(|v| v.as_str()) == Some(mode))
            .unwrap_or_else(|| panic!("{} mode missing from {}", mode, ux_path.display()));
        let samples = run
            .get("samples")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("{} samples missing from {}", mode, ux_path.display()));
        assert!(
            samples.len() >= 10,
            "{} must have at least 10 {} samples",
            ux_path.display(),
            mode
        );
        startup_samples += samples.len();
        for field in ["avg_startup_total_time_ms", "p95_startup_total_time_ms"] {
            assert!(
                run.get("summary")
                    .and_then(|v| v.get(field))
                    .and_then(|v| v.as_f64())
                    .is_some(),
                "{} summary.{} missing from {}",
                mode,
                field,
                ux_path.display()
            );
        }
    }
    assert!(
        startup_samples >= 20,
        "{} must have at least 20 startup samples across cold/prewarm modes",
        ux_path.display()
    );
}

#[test]
fn emulator_ux_gross_saved_beats_strong_baseline() {
    let report_path = find_report().expect("lsapp-standard.report.json fixture must be committed");
    let report = load_json(&report_path).expect("lsapp-standard.report.json must parse");
    let ux_path = find_ux_metrics().expect("ux-metrics fixture must be committed");
    let ux = load_json(&ux_path).expect("ux-metrics fixture must parse");

    let ensemble_hit = report
        .get("metrics")
        .and_then(|m| m.get("ensemble"))
        .and_then(|e| e.get("hit_rate_at_1_pct"))
        .and_then(|v| v.as_f64())
        .expect("ensemble hit_rate_at_1_pct missing from report");
    let strong_hit = report
        .get("metrics")
        .and_then(|m| m.get("strong_predictive"))
        .and_then(|e| e.get("hit_rate_at_1_pct"))
        .and_then(|v| v.as_f64())
        .expect("strong_predictive hit_rate_at_1_pct missing from report");
    let examples = report
        .get("test_examples")
        .and_then(|v| v.as_u64())
        .expect("test_examples missing from report") as f64;
    let saved_ms = ux
        .get("ux_deltas")
        .and_then(|d| d.get("prewarm_vs_cold"))
        .and_then(|p| p.get("startup_total_time_ms_reduction"))
        .and_then(|v| v.as_f64())
        .expect("ux prewarm_vs_cold startup reduction missing");

    let ensemble_gross = examples * ensemble_hit * saved_ms / 100.0;
    let strong_gross = examples * strong_hit * saved_ms / 100.0;
    assert!(
        ensemble_gross > 0.0,
        "DiPECS ensemble gross saved latency should be positive"
    );
    assert!(
        ensemble_gross > strong_gross,
        "DiPECS ensemble gross saved latency ({ensemble_gross:.0} ms) should beat strong baseline ({strong_gross:.0} ms)"
    );
}

/// #90 gate: use committed LSApp standard hit@1 plus Pixel 6a n>=20
/// PreWarm hit/miss measurements to recompute action-level net benefit.
#[test]
fn dipecs_ensemble_net_benefit_beats_strong_baseline() {
    let report_path = find_report().expect("lsapp-standard.report.json fixture must be committed");
    let report = load_json(&report_path).expect("lsapp-standard.report.json must parse");

    let ensemble_hit = report
        .get("metrics")
        .and_then(|m| m.get("ensemble"))
        .and_then(|e| e.get("hit_rate_at_1_pct"))
        .and_then(|v| v.as_f64())
        .expect("ensemble hit_rate_at_1_pct missing from report");
    assert!(
        ensemble_hit > 0.0,
        "ensemble hit_rate_at_1_pct should be positive"
    );

    let strong_hit = match report
        .get("metrics")
        .and_then(|m| m.get("strong_predictive"))
        .and_then(|e| e.get("hit_rate_at_1_pct"))
        .and_then(|v| v.as_f64())
    {
        Some(h) => h,
        None => panic!(
            "strong_predictive hit_rate_at_1_pct missing from {}; \
             regenerate the report with the strong baseline enabled",
            report_path.display()
        ),
    };

    let examples = report
        .get("test_examples")
        .and_then(|v| v.as_u64())
        .expect("test_examples missing from report") as usize;

    let net_path =
        find_net_benefit_report().expect("PreWarm net-benefit fixture must be committed");
    let net = load_json(&net_path).expect("PreWarm net-benefit fixture must parse");
    assert_eq!(
        net.get("schema_version").and_then(|v| v.as_str()),
        Some("dipecs.prewarm_net_benefit.v1"),
        "{} must be the PreWarm net-benefit schema",
        net_path.display()
    );
    assert_eq!(
        net.get("source").and_then(|v| v.as_str()),
        Some("measured_device"),
        "{} must be tagged as measured_device",
        net_path.display()
    );
    assert_eq!(
        net.get("status").and_then(|v| v.as_str()),
        Some("measured_android_real_device"),
        "{} must be a real-device measurement",
        net_path.display()
    );
    assert_eq!(
        net.get("net_benefit")
            .and_then(|v| v.get("examples"))
            .and_then(|v| v.as_u64()),
        Some(examples as u64),
        "net-benefit fixture must use the LSApp standard test example count"
    );

    let runs = net
        .get("runs")
        .and_then(|v| v.as_array())
        .expect("net-benefit runs must be present");
    for expected_mode in [
        "collector_cold_startup",
        "collector_prewarm_hit_startup",
        "settings_cold_startup",
        "settings_after_wrong_prewarm_startup",
    ] {
        let run = runs
            .iter()
            .find(|run| run.get("mode").and_then(|v| v.as_str()) == Some(expected_mode))
            .unwrap_or_else(|| panic!("{expected_mode} run missing from {}", net_path.display()));
        let samples = run
            .get("samples")
            .and_then(|v| v.as_array())
            .expect("run samples must be present");
        assert!(
            samples.len() >= 20,
            "{expected_mode} must have n>=20 samples in {}",
            net_path.display()
        );
        let summary = run.get("summary").expect("run summary must be present");
        assert_eq!(
            summary.get("n").and_then(|v| v.as_u64()),
            Some(samples.len() as u64),
            "{expected_mode} summary.n must match sample count"
        );
        assert!(
            summary
                .get("mean_startup_total_time_ms")
                .and_then(|v| v.as_f64())
                .is_some(),
            "{expected_mode} mean startup must be present"
        );
        assert!(
            summary
                .get("p95_startup_total_time_ms")
                .and_then(|v| v.as_f64())
                .is_some(),
            "{expected_mode} p95 startup must be present"
        );
    }

    let summary_value = |mode: &str, field: &str| -> f64 {
        runs.iter()
            .find(|run| run.get("mode").and_then(|v| v.as_str()) == Some(mode))
            .and_then(|run| run.get("summary"))
            .and_then(|summary| summary.get(field))
            .and_then(|v| v.as_f64())
            .unwrap_or_else(|| panic!("{mode} summary.{field} missing from {}", net_path.display()))
    };
    let recomputed_saved_ms = summary_value("collector_cold_startup", "mean_startup_total_time_ms")
        - summary_value(
            "collector_prewarm_hit_startup",
            "mean_startup_total_time_ms",
        );
    let recomputed_miss_delta_ms =
        summary_value(
            "settings_after_wrong_prewarm_startup",
            "mean_startup_total_time_ms",
        ) - summary_value("settings_cold_startup", "mean_startup_total_time_ms");
    let recomputed_control_ms =
        summary_value("collector_prewarm_hit_startup", "mean_prewarm_latency_us") / 1000.0;

    let measured = net
        .get("measured_inputs")
        .expect("measured_inputs must be present");
    assert_eq!(
        measured.get("source").and_then(|v| v.as_str()),
        Some("measured_device"),
        "measured inputs must carry measured_device provenance"
    );
    let saved_ms = measured
        .get("hit_saved_ms")
        .and_then(|v| v.as_f64())
        .expect("hit_saved_ms missing from measured_inputs");
    let miss_cost_ms = measured
        .get("miss_action_cost_ms")
        .and_then(|v| v.as_f64())
        .expect("miss_action_cost_ms missing from measured_inputs");
    let miss_delta_ms = measured
        .get("miss_startup_delta_ms")
        .and_then(|v| v.as_f64())
        .expect("miss_startup_delta_ms missing from measured_inputs");
    let mean_prewarm_latency_ms = measured
        .get("mean_prewarm_latency_ms")
        .and_then(|v| v.as_f64())
        .expect("mean_prewarm_latency_ms missing from measured_inputs");
    let control_plane_ms = measured
        .get("control_plane_ms")
        .and_then(|v| v.as_f64())
        .expect("control_plane_ms missing from measured_inputs");
    const MEASURED_INPUT_TOLERANCE_MS: f64 = 0.001;
    assert!(
        (saved_ms - recomputed_saved_ms).abs() <= MEASURED_INPUT_TOLERANCE_MS,
        "hit_saved_ms must match run summaries"
    );
    assert!(
        (miss_delta_ms - recomputed_miss_delta_ms).abs() <= MEASURED_INPUT_TOLERANCE_MS,
        "miss_startup_delta_ms must match wrong-prewarm run summaries"
    );
    assert!(
        (miss_cost_ms - recomputed_miss_delta_ms.max(0.0)).abs() <= MEASURED_INPUT_TOLERANCE_MS,
        "miss_action_cost_ms must match the non-negative wrong-prewarm startup delta"
    );
    assert!(
        (mean_prewarm_latency_ms - recomputed_control_ms).abs() <= MEASURED_INPUT_TOLERANCE_MS,
        "mean_prewarm_latency_ms must match run summaries"
    );
    assert!(
        (control_plane_ms - recomputed_control_ms).abs() <= MEASURED_INPUT_TOLERANCE_MS,
        "control_plane_ms must match measured PreWarm dispatch latency"
    );
    assert!(
        saved_ms > 0.0,
        "measured PreWarm saved latency must be positive"
    );
    assert!(
        miss_cost_ms >= 0.0,
        "measured miss action cost must be non-negative"
    );
    assert!(
        control_plane_ms > 0.0,
        "measured control-plane/dispatch cost must be positive"
    );

    let ensemble_inputs = NetBenefitInputs {
        hit_rate_at_1_pct: ensemble_hit as f32,
        prewarm_saved_ms: saved_ms,
        prewarm_wasted_ms: miss_cost_ms,
        control_plane_ms,
    };
    let strong_inputs = NetBenefitInputs {
        hit_rate_at_1_pct: strong_hit as f32,
        prewarm_saved_ms: saved_ms,
        prewarm_wasted_ms: miss_cost_ms,
        control_plane_ms,
    };

    let ensemble_report = compute_net_benefit(&ensemble_inputs, examples);
    let strong_report = compute_net_benefit(&strong_inputs, examples);

    let recorded_ensemble = net
        .get("net_benefit")
        .and_then(|v| v.get("dipecs_ensemble"))
        .and_then(|v| v.get("net_benefit_ms"))
        .and_then(|v| v.as_f64())
        .expect("recorded DiPECS net_benefit_ms missing");
    let recorded_strong = net
        .get("net_benefit")
        .and_then(|v| v.get("strong_predictive"))
        .and_then(|v| v.get("net_benefit_ms"))
        .and_then(|v| v.as_f64())
        .expect("recorded strong_predictive net_benefit_ms missing");

    // Recorded values are rounded in JSON and hit rates pass through f32 in
    // NetBenefitInputs, so allow a small absolute tolerance on a 76M ms total.
    const RECORDED_NET_BENEFIT_TOLERANCE_MS: f64 = 20.0;
    assert!(
        (ensemble_report.net_benefit_ms - recorded_ensemble).abs()
            <= RECORDED_NET_BENEFIT_TOLERANCE_MS,
        "recorded DiPECS net benefit must match recomputation"
    );
    assert!(
        (strong_report.net_benefit_ms - recorded_strong).abs() <= RECORDED_NET_BENEFIT_TOLERANCE_MS,
        "recorded strong baseline net benefit must match recomputation"
    );
    assert!(
        ensemble_report.net_benefit_ms > 0.0,
        "DiPECS ensemble net benefit must be positive; got {:.3} ms",
        ensemble_report.net_benefit_ms
    );
    assert!(
        ensemble_report.net_benefit_ms > strong_report.net_benefit_ms,
        "DiPECS ensemble net benefit ({:.3} ms) must beat strong baseline ({:.3} ms)",
        ensemble_report.net_benefit_ms,
        strong_report.net_benefit_ms
    );
}

#[test]
fn prewarm_net_benefit_conclusion_matches_gates() {
    let net_path =
        find_net_benefit_report().expect("PreWarm net-benefit fixture must be committed");
    let net = load_json(&net_path).expect("PreWarm net-benefit fixture must parse");
    let conclusion = net
        .get("conclusion")
        .expect("net-benefit conclusion must be present");

    let n_gate = conclusion
        .get("n_at_least_20_per_mode")
        .and_then(|v| v.as_bool())
        .expect("n_at_least_20_per_mode must be recorded");
    let inputs_gate = conclusion
        .get("measured_inputs_valid")
        .and_then(|v| v.as_bool())
        .expect("measured_inputs_valid must be recorded");
    let positive_gate = conclusion
        .get("net_benefit_positive")
        .and_then(|v| v.as_bool())
        .expect("net_benefit_positive must be recorded");
    let beats_gate = conclusion
        .get("dipecs_beats_strong_predictive")
        .and_then(|v| v.as_bool())
        .expect("dipecs_beats_strong_predictive must be recorded");

    assert_eq!(
        conclusion.get("accepted").and_then(|v| v.as_bool()),
        Some(n_gate && inputs_gate && positive_gate && beats_gate),
        "accepted must be derived from all net-benefit gates"
    );
}
