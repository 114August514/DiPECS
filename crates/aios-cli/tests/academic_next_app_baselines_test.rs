use std::fs;
use std::path::PathBuf;

use serde_json::Value;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn baselines_path() -> PathBuf {
    workspace_root()
        .join("data")
        .join("evaluation")
        .join("academic-next-app-baselines.json")
}

fn lsapp_standard_report_path() -> PathBuf {
    workspace_root()
        .join("data")
        .join("evaluation")
        .join("next-app")
        .join("lsapp-standard.report.json")
}

fn load_json(path: PathBuf) -> Value {
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("could not read {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("could not parse {}: {err}", path.display()))
}

fn non_empty_str<'a>(value: &'a Value, field: &str) -> &'a str {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| panic!("{field} must be a non-empty string in {value:?}"))
}

fn optional_metric(metrics: &Value, field: &str) -> Option<f64> {
    match metrics.get(field) {
        Some(Value::Null) | None => None,
        Some(value) => Some(
            value
                .as_f64()
                .unwrap_or_else(|| panic!("{field} must be numeric or null: {metrics:?}")),
        ),
    }
}

#[test]
fn academic_baseline_fixture_has_maintainable_schema() {
    let fixture = load_json(baselines_path());
    assert_eq!(
        fixture.get("schema_version").and_then(Value::as_str),
        Some("dipecs.academic_next_app_baselines.v1")
    );
    assert_eq!(
        fixture.get("last_updated").and_then(Value::as_str),
        Some("2026-07-04")
    );

    let baselines = fixture
        .get("baselines")
        .and_then(Value::as_array)
        .expect("baselines must be an array");
    assert!(
        baselines.len() >= 8,
        "issue #102 should track DiPECS plus multiple academic baseline families"
    );

    let mut saw_maple = false;
    let mut saw_appformer = false;
    let mut saw_poi = false;
    let mut saw_gnn = false;
    let mut saw_happ_or_appredict = false;

    for baseline in baselines {
        let method = non_empty_str(baseline, "method");
        non_empty_str(baseline, "paper");
        non_empty_str(baseline, "dataset");
        non_empty_str(baseline, "split");
        non_empty_str(baseline, "metric_scope");
        non_empty_str(baseline, "comparability_note");
        let source_url = non_empty_str(baseline, "source_url");
        assert!(
            source_url.starts_with("http") || source_url.starts_with("data/"),
            "source_url must be a URL or committed data path: {source_url}"
        );
        let source_locator = non_empty_str(baseline, "source_locator");

        let comparability = non_empty_str(baseline, "comparability");
        assert!(
            ["direct", "contextual_only", "excluded_unclear"].contains(&comparability),
            "unexpected comparability value {comparability}"
        );

        let metrics = baseline
            .get("metrics")
            .unwrap_or_else(|| panic!("{method} missing metrics object"));
        let mut numeric_metrics = 0usize;
        for field in [
            "hit_at_1_pct",
            "hit_at_3_pct",
            "hit_at_5_pct",
            "mrr_at_5_pct",
            "macro_hit_at_1_pct",
        ] {
            if let Some(metric) = optional_metric(metrics, field) {
                numeric_metrics += 1;
                assert!(
                    metric.is_finite() && (0.0..=100.0).contains(&metric),
                    "{method} {field} must be a finite percentage in [0, 100], got {metric}"
                );
                assert!(
                    !source_locator.eq_ignore_ascii_case("unknown"),
                    "{method} numeric metrics need a precise source locator"
                );
            }
        }

        let comparable = baseline
            .get("reported_comparable_to_dipecs")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| panic!("{method} missing reported_comparable_to_dipecs"));
        if comparable {
            assert_eq!(
                comparability, "direct",
                "{method} cannot be reported comparable unless comparability=direct"
            );
            assert_eq!(
                baseline.get("dataset").and_then(Value::as_str),
                Some("LSApp"),
                "{method} direct rows must use LSApp"
            );
            assert!(
                numeric_metrics > 0,
                "{method} direct rows must carry at least one metric"
            );
        } else {
            assert_ne!(
                comparability, "direct",
                "{method} direct rows must opt in with reported_comparable_to_dipecs=true"
            );
        }

        saw_maple |= method.contains("MAPLE");
        saw_appformer |= method.contains("AppFormer");
        saw_poi |= method.contains("POI") || method.contains("PAULCI");
        saw_gnn |= method.contains("GNN");
        saw_happ_or_appredict |= method.contains("HAPP") || method.contains("APPredict");
    }

    assert!(saw_maple, "fixture should track MAPLE");
    assert!(saw_appformer, "fixture should track AppFormer");
    assert!(saw_poi, "fixture should track POI-style baselines");
    assert!(saw_gnn, "fixture should track GNN-based baselines");
    assert!(
        saw_happ_or_appredict,
        "fixture should explicitly track HAPP / APPredict follow-up status"
    );
}

#[test]
fn dipecs_reference_matches_committed_lsapp_standard_report() {
    let fixture = load_json(baselines_path());
    let report = load_json(lsapp_standard_report_path());
    let reference = fixture
        .get("dipecs_reference")
        .expect("dipecs_reference missing");
    let reference_metrics = reference
        .get("metrics")
        .expect("dipecs_reference.metrics missing");
    let report_ensemble = report
        .get("metrics")
        .and_then(|metrics| metrics.get("ensemble"))
        .expect("report ensemble metrics missing");

    assert_eq!(
        reference.get("report_path").and_then(Value::as_str),
        Some("data/evaluation/next-app/lsapp-standard.report.json")
    );
    assert_eq!(
        reference.get("test_examples").and_then(Value::as_u64),
        report.get("test_examples").and_then(Value::as_u64)
    );

    let metric_pairs = [
        ("hit_at_1_pct", "hit_rate_at_1_pct", 1.0),
        ("hit_at_3_pct", "hit_rate_at_3_pct", 1.0),
        ("hit_at_5_pct", "hit_rate_at_5_pct", 1.0),
        ("mrr_at_5_pct", "mean_reciprocal_rank_at_5", 100.0),
        ("macro_hit_at_1_pct", "macro_hit_rate_at_1_pct", 1.0),
    ];
    for (fixture_field, report_field, multiplier) in metric_pairs {
        let fixture_value = reference_metrics
            .get(fixture_field)
            .and_then(Value::as_f64)
            .unwrap_or_else(|| panic!("dipecs_reference metric {fixture_field} missing"));
        let report_value = report_ensemble
            .get(report_field)
            .and_then(Value::as_f64)
            .unwrap_or_else(|| panic!("report ensemble metric {report_field} missing"))
            * multiplier;
        assert!(
            (fixture_value - report_value).abs() < 0.001,
            "{fixture_field} ({fixture_value}) must match report {report_field} ({report_value})"
        );
    }
}

#[test]
fn academic_baseline_doc_is_wired_and_mentions_comparability_policy() {
    let root = workspace_root();
    let doc_path = root
        .join("docs")
        .join("src")
        .join("evaluation")
        .join("academic-baseline-comparison.md");
    let doc = fs::read_to_string(&doc_path)
        .unwrap_or_else(|err| panic!("could not read {}: {err}", doc_path.display()));
    assert!(doc.contains("data/evaluation/academic-next-app-baselines.json"));
    assert!(doc.contains("不要这样使用"));
    assert!(doc.contains("direct"));
    assert!(doc.contains("contextual_only") || doc.contains("仅作背景"));
    assert!(doc.contains("MAPLE"));
    assert!(doc.contains("AppFormer"));
    assert!(doc.contains("POI"));
    assert!(doc.contains("GNN"));

    let nav = fs::read_to_string(root.join("docs").join("mkdocs.yml"))
        .expect("docs/mkdocs.yml should be readable");
    assert!(
        nav.contains("evaluation/academic-baseline-comparison.md"),
        "academic baseline doc must be linked from mkdocs nav"
    );
}
