use std::fs;

use aios_cli::next_app::{compute_measured_net_benefit, PrewarmNetBenefitFixture};

use super::support::{find_prewarm_net_benefit_fixture, find_report, load_json};

#[test]
fn offline_prewarm_net_benefit_fixture_has_measured_provenance() {
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
    assert_eq!(
        fixture.measurements.wasted_prewarm.samples, 1,
        "offline fixture records the current single-sample ack-latency approximation"
    );
    let note = fixture
        .measurements
        .wasted_prewarm
        .source
        .note
        .as_deref()
        .unwrap_or_default();
    assert!(
        note.contains("wrong-target"),
        "offline fixture provenance must keep the wrong-target replacement caveat"
    );
}

#[test]
fn dipecs_offline_fixture_net_benefit_beats_strong_baseline() {
    let report_path = find_report().expect("lsapp-standard.report.json fixture must be committed");
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
        .expect("ensemble hit_rate_at_1_pct missing from report") as f32;
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
