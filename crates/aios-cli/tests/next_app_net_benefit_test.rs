//! Integration test: DiPECS ensemble next-app net benefit vs. strong predictive baseline.
//!
//! The test uses committed evaluation fixtures:
//! - `data/evaluation/lsapp-standard.report.json` for hit rates and example count.
//! - The latest `data/evaluation/ux-metrics-emulator-*.json` for the measured PreWarm
//!   startup time reduction.
//!
//! If either fixture is missing or does not contain the expected fields, the test
//! skips loudly rather than failing.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use aios_cli::next_app::{
    compute_measured_net_benefit, compute_net_benefit, NetBenefitInputs, PrewarmNetBenefitFixture,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn evaluation_dir() -> PathBuf {
    workspace_root().join("data/evaluation")
}

fn find_report() -> Option<PathBuf> {
    let path = evaluation_dir()
        .join("next-app")
        .join("lsapp-standard.report.json");
    path.exists().then_some(path)
}

fn find_ux_metrics() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = fs::read_dir(evaluation_dir().join("ux-metrics"))
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with("ux-metrics-emulator-") && name.ends_with(".json") {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();
    candidates.sort();
    candidates.into_iter().last()
}
fn find_prewarm_net_benefit_fixture() -> Option<PathBuf> {
    let path = evaluation_dir()
        .join("action-net-benefit")
        .join("prewarm-emulator-20260704-measured-v1.json");
    path.exists().then_some(path)
}
fn load_json(path: &PathBuf) -> Option<serde_json::Value> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("SKIP: could not read {}: {e}", path.display());
            return None;
        },
    };
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("SKIP: could not parse {}: {e}", path.display());
            None
        },
    }
}

fn unique_tmp_fixture_path() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join(format!("dipecs-prewarm-net-benefit-{suffix}"))
        .join("fixture.json")
}

fn unique_tmp_dir(prefix: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{suffix}"))
}

fn run_fixture_generator(report: &PathBuf, ux_metrics: &PathBuf, output: &PathBuf) -> bool {
    Command::new(env!("CARGO_BIN_EXE_aios-cli"))
        .arg("generate-prewarm-net-benefit-fixture")
        .arg("--report")
        .arg(report)
        .arg("--ux-metrics")
        .arg(ux_metrics)
        .arg("--output")
        .arg(output)
        .arg("--wasted-prewarm-ms")
        .arg("31.231")
        .arg("--wasted-prewarm-samples")
        .arg("1")
        .arg("--dipecs-control-plane-ms")
        .arg("0.07848")
        .arg("--dipecs-control-plane-samples")
        .arg("1631")
        .arg("--strong-control-plane-ms")
        .arg("0.0")
        .arg("--strong-control-plane-samples")
        .arg("272519")
        .status()
        .expect("aios-cli should run")
        .success()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsapp_report_contains_strong_predictive_baseline() {
        let report_path =
            find_report().expect("lsapp-standard.report.json fixture must be committed");
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
        let report_path =
            find_report().expect("lsapp-standard.report.json fixture must be committed");
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

    /// Honest assertions on gross saved latency, which is computed from fully
    /// measured inputs only:
    /// - real `hit_rate_at_1_pct` from the LSApp report
    /// - real `prewarm_saved_ms` from the UX fixture
    ///
    /// This is not the full #90 net-benefit gate: wasted action cost and
    /// control-plane overhead are still placeholders below. It is a real,
    /// non-skipping gate for the measured gross-saved slice that #90/#91 depend
    /// on: DiPECS must first beat the strong predictive baseline on the same
    /// LSApp hit@1 window before action-cost accounting can make a stronger
    /// system claim.
    #[test]
    fn dipecs_ensemble_gross_saved_beats_strong_baseline() {
        let report_path =
            find_report().expect("lsapp-standard.report.json fixture must be committed");
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

        let ux_path = find_ux_metrics().expect("ux-metrics fixture must be committed");
        let ux = load_json(&ux_path).expect("ux-metrics fixture must parse");
        let saved_ms = ux
            .get("ux_deltas")
            .and_then(|d| d.get("prewarm_vs_cold"))
            .and_then(|p| p.get("startup_total_time_ms_reduction"))
            .and_then(|v| v.as_f64())
            .unwrap_or_else(|| {
                panic!(
                    "ux_deltas.prewarm_vs_cold.startup_total_time_ms_reduction missing from {}",
                    ux_path.display()
                )
            });

        // Gross saved latency uses only measured inputs, so we assert on it.
        let examples_f = examples as f64;
        let ensemble_gross = examples_f * ensemble_hit * saved_ms / 100.0;
        let strong_gross = examples_f * strong_hit * saved_ms / 100.0;

        assert!(
            ensemble_gross > 0.0,
            "DiPECS ensemble gross saved latency should be positive; got {} ms",
            ensemble_gross
        );
        assert!(
            ensemble_gross > strong_gross,
            "DiPECS ensemble gross saved latency ({:.0} ms) should be strictly greater than strong baseline ({:.0} ms)",
            ensemble_gross,
            strong_gross
        );

        // The current UX fixtures measure the cold-to-prewarm saving, but do not yet
        // expose a dedicated missed-prewarm cost or control-plane overhead. Build
        // the inputs through the placeholder constructor so those two unmeasured
        // fields are impossible to mistake for real data (see TODO(#90) on
        // NetBenefitInputs::placeholder_pending_measurement). We keep the
        // net-benefit arithmetic exercised end-to-end, but only log the result;
        // net-benefit assertions stay disabled until those measurements exist.
        let ensemble_inputs =
            NetBenefitInputs::placeholder_pending_measurement(ensemble_hit as f32, saved_ms);
        let strong_inputs =
            NetBenefitInputs::placeholder_pending_measurement(strong_hit as f32, saved_ms);

        let ensemble_report = compute_net_benefit(&ensemble_inputs, examples);
        let strong_report = compute_net_benefit(&strong_inputs, examples);

        eprintln!(
            "ensemble net_benefit_ms={} (PLACEHOLDER wasted/control costs, not measured)",
            ensemble_report.net_benefit_ms
        );
        eprintln!(
            "strong baseline net_benefit_ms={} (PLACEHOLDER wasted/control costs, not measured)",
            strong_report.net_benefit_ms
        );
    }
    #[test]
    fn prewarm_net_benefit_fixture_is_fully_measured() {
        let fixture_path = find_prewarm_net_benefit_fixture()
            .expect("prewarm action-net-benefit fixture must be committed");
        let fixture: PrewarmNetBenefitFixture =
            serde_json::from_reader(fs::File::open(&fixture_path).expect("fixture should open"))
                .expect("prewarm action-net-benefit fixture must parse");

        fixture
            .validate()
            .expect("prewarm action-net-benefit fixture must contain measured inputs");
        assert_eq!(fixture.action, "PreWarmProcess");
        assert_eq!(fixture.trace.split, "Standard");
        assert!(
            !fixture.status.to_ascii_lowercase().contains("placeholder"),
            "fixture status must not describe placeholder data"
        );
        assert!(
            fixture.measurements.prewarm_saved.samples >= 20,
            "prewarm saved latency must keep the 10 cold + 10 prewarm measured sample count"
        );
        assert!(
            fixture.measurements.wasted_prewarm.mean_ms > 0.0,
            "wasted prewarm cost must be measured and non-zero"
        );
    }

    #[test]
    fn dipecs_measured_prewarm_net_benefit_beats_strong_baseline() {
        let report_path =
            find_report().expect("lsapp-standard.report.json fixture must be committed");
        let report = load_json(&report_path).expect("lsapp-standard.report.json must parse");
        let fixture_path = find_prewarm_net_benefit_fixture()
            .expect("prewarm action-net-benefit fixture must be committed");
        let fixture: PrewarmNetBenefitFixture =
            serde_json::from_reader(fs::File::open(&fixture_path).expect("fixture should open"))
                .expect("prewarm action-net-benefit fixture must parse");
        fixture.validate().expect("fixture must be measured");

        let examples = report
            .get("test_examples")
            .and_then(|v| v.as_u64())
            .expect("test_examples missing from report") as usize;
        assert_eq!(
            fixture.trace.examples, examples,
            "net-benefit fixture must use the same LSApp Standard test window count"
        );

        let ensemble_hit = report
            .get("metrics")
            .and_then(|m| m.get("ensemble"))
            .and_then(|e| e.get("hit_rate_at_1_pct"))
            .and_then(|v| v.as_f64())
            .expect("ensemble hit_rate_at_1_pct missing from report")
            as f32;
        let strong_hit = report
            .get("metrics")
            .and_then(|m| m.get("strong_predictive"))
            .and_then(|e| e.get("hit_rate_at_1_pct"))
            .and_then(|v| v.as_f64())
            .expect("strong_predictive hit_rate_at_1_pct missing from report")
            as f32;

        let ensemble_report =
            compute_measured_net_benefit(&fixture.dipecs_inputs(ensemble_hit), examples)
                .expect("DiPECS measured net benefit should compute");
        let strong_report =
            compute_measured_net_benefit(&fixture.strong_baseline_inputs(strong_hit), examples)
                .expect("strong baseline measured net benefit should compute");

        assert!(
            ensemble_report.net_benefit_ms > 0.0,
            "DiPECS measured PreWarm net benefit should be positive; got {:.0} ms",
            ensemble_report.net_benefit_ms
        );
        assert!(
            ensemble_report.net_benefit_ms > strong_report.net_benefit_ms,
            "DiPECS measured PreWarm net benefit ({:.0} ms) should beat strong baseline ({:.0} ms)",
            ensemble_report.net_benefit_ms,
            strong_report.net_benefit_ms
        );
    }

    #[test]
    fn cli_generates_valid_prewarm_net_benefit_fixture() {
        let report_path =
            find_report().expect("lsapp-standard.report.json fixture must be committed");
        let ux_path = find_ux_metrics().expect("ux-metrics fixture must be committed");
        let output = unique_tmp_fixture_path();

        let status = Command::new(env!("CARGO_BIN_EXE_aios-cli"))
            .arg("generate-prewarm-net-benefit-fixture")
            .arg("--report")
            .arg(&report_path)
            .arg("--ux-metrics")
            .arg(&ux_path)
            .arg("--output")
            .arg(&output)
            .arg("--dataset-id")
            .arg("prewarm-cli-generated-test")
            .arg("--wasted-prewarm-ms")
            .arg("31.231")
            .arg("--wasted-prewarm-samples")
            .arg("1")
            .arg("--dipecs-control-plane-ms")
            .arg("0.07848")
            .arg("--dipecs-control-plane-samples")
            .arg("1631")
            .arg("--strong-control-plane-ms")
            .arg("0.0")
            .arg("--strong-control-plane-samples")
            .arg("272519")
            .status()
            .expect("aios-cli should run");
        assert!(
            status.success(),
            "fixture generator should exit successfully"
        );

        let fixture: PrewarmNetBenefitFixture =
            serde_json::from_reader(fs::File::open(&output).expect("fixture should open"))
                .expect("generated fixture must parse");
        fixture.validate().expect("generated fixture must validate");
        assert_eq!(fixture.dataset_id, "prewarm-cli-generated-test");
        assert_eq!(fixture.trace.split, "Standard");
        assert!(fixture.measurements.prewarm_saved.samples >= 20);
    }

    #[test]
    fn cli_rejects_negative_wasted_prewarm_cost() {
        let report_path =
            find_report().expect("lsapp-standard.report.json fixture must be committed");
        let ux_path = find_ux_metrics().expect("ux-metrics fixture must be committed");
        let output = unique_tmp_fixture_path();

        let status = Command::new(env!("CARGO_BIN_EXE_aios-cli"))
            .arg("generate-prewarm-net-benefit-fixture")
            .arg("--report")
            .arg(&report_path)
            .arg("--ux-metrics")
            .arg(&ux_path)
            .arg("--output")
            .arg(&output)
            .arg("--wasted-prewarm-ms")
            .arg("-1.0")
            .arg("--dipecs-control-plane-ms")
            .arg("0.07848")
            .status()
            .expect("aios-cli should run");

        assert!(
            !status.success(),
            "negative wasted-prewarm cost must fail validation"
        );
        assert!(
            !output.exists(),
            "failed fixture generation must not leave an output fixture"
        );
    }

    #[test]
    fn cli_rejects_ux_metrics_without_prewarm_delta() {
        let report_path =
            find_report().expect("lsapp-standard.report.json fixture must be committed");
        let tmp = unique_tmp_dir("dipecs-bad-ux");
        fs::create_dir_all(&tmp).expect("tmp dir should be created");
        let bad_ux = tmp.join("bad-ux.json");
        fs::write(
            &bad_ux,
            r#"{"schema_version":"dipecs.ux_metrics.v1","runs":[]}"#,
        )
        .expect("bad UX fixture should be written");
        let output = tmp.join("fixture.json");

        let status = Command::new(env!("CARGO_BIN_EXE_aios-cli"))
            .arg("generate-prewarm-net-benefit-fixture")
            .arg("--report")
            .arg(&report_path)
            .arg("--ux-metrics")
            .arg(&bad_ux)
            .arg("--output")
            .arg(&output)
            .arg("--wasted-prewarm-ms")
            .arg("31.231")
            .arg("--dipecs-control-plane-ms")
            .arg("0.07848")
            .status()
            .expect("aios-cli should run");

        assert!(
            !status.success(),
            "missing prewarm delta must fail fixture generation"
        );
        assert!(
            !output.exists(),
            "failed fixture generation must not leave an output fixture"
        );
    }

    #[test]
    fn cli_rejects_many_corrupt_report_and_ux_fixtures() {
        let tmp = unique_tmp_dir("dipecs-corrupt-fixtures");
        fs::create_dir_all(&tmp).expect("tmp dir should be created");
        let good_report = tmp.join("good-report.json");
        fs::write(
            &good_report,
            r#"{"split":"Standard","test_examples":272519,"metrics":{"ensemble":{"hit_rate_at_1_pct":56.442},"strong_predictive":{"hit_rate_at_1_pct":53.784}}}"#,
        )
        .expect("good report should be written");
        let good_ux = tmp.join("good-ux.json");
        fs::write(
            &good_ux,
            r#"{
                "ux_deltas":{"prewarm_vs_cold":{"startup_total_time_ms_reduction":394.8}},
                "runs":[
                    {"mode":"cold_startup","samples":[{},{}],"summary":{"p95_startup_total_time_ms":932.0}},
                    {"mode":"prewarm_startup","samples":[{},{}],"summary":{"p95_startup_total_time_ms":512.0}}
                ]
            }"#,
        )
        .expect("good UX should be written");

        let report_cases = [
            ("missing_split", r#"{"test_examples":1}"#),
            ("missing_examples", r#"{"split":"Standard"}"#),
            ("zero_examples", r#"{"split":"Standard","test_examples":0}"#),
            (
                "string_examples",
                r#"{"split":"Standard","test_examples":"272519"}"#,
            ),
        ];
        for (name, json) in report_cases {
            let report = tmp.join(format!("{name}.report.json"));
            fs::write(&report, json).expect("bad report should be written");
            let output = tmp.join(format!("{name}.fixture.json"));
            assert!(
                !run_fixture_generator(&report, &good_ux, &output),
                "{name} report should be rejected"
            );
            assert!(!output.exists(), "{name} must not leave output");
        }

        let ux_cases = [
            ("missing_deltas", r#"{"runs":[]}"#),
            (
                "string_delta",
                r#"{"ux_deltas":{"prewarm_vs_cold":{"startup_total_time_ms_reduction":"394.8"}},"runs":[]}"#,
            ),
            (
                "missing_runs_allowed_but_delta_present",
                r#"{"ux_deltas":{"prewarm_vs_cold":{"startup_total_time_ms_reduction":394.8}}}"#,
            ),
            (
                "negative_delta",
                r#"{"ux_deltas":{"prewarm_vs_cold":{"startup_total_time_ms_reduction":-1.0}},"runs":[]}"#,
            ),
        ];
        for (name, json) in ux_cases {
            let ux = tmp.join(format!("{name}.ux.json"));
            fs::write(&ux, json).expect("UX fixture should be written");
            let output = tmp.join(format!("{name}.fixture.json"));
            let success = run_fixture_generator(&good_report, &ux, &output);
            if name == "missing_runs_allowed_but_delta_present" {
                assert!(success, "{name} should fall back to one sample");
                assert!(output.exists(), "{name} should write output");
            } else {
                assert!(!success, "{name} UX should be rejected");
                assert!(!output.exists(), "{name} must not leave output");
            }
        }
    }
}
