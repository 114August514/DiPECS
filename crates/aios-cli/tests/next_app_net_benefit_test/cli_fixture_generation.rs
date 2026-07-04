use std::fs;
use std::process::Command;

use aios_cli::next_app::PrewarmNetBenefitFixture;

use super::support::{
    find_report, find_ux_metrics, run_fixture_generator, unique_tmp_dir, unique_tmp_fixture_path,
};

#[test]
fn cli_generates_valid_prewarm_net_benefit_fixture() {
    let report_path = find_report().expect("lsapp-standard.report.json fixture must be committed");
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
    let report_path = find_report().expect("lsapp-standard.report.json fixture must be committed");
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
    let report_path = find_report().expect("lsapp-standard.report.json fixture must be committed");
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
