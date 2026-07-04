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

use aios_cli::next_app::{compute_net_benefit, NetBenefitInputs};

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

        // The current UX fixtures measure the cold→prewarm saving, but do not yet
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
}
