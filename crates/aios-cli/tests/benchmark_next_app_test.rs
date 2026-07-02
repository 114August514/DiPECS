//! Regression tests for the next-app prediction benchmark.

use std::path::PathBuf;

use aios_cli::benchmark_next_app::baselines::{
    FirstCandidateBackend, MarkovBackend, PerCurrentAppMajorityBackend, RandomCandidateBackend,
};
use aios_cli::benchmark_next_app::runner::{run_benchmark, BenchmarkRunConfig};
use aios_cli::benchmark_next_app::types::NextAppPredictor;
use aios_spec::{
    AppTransition, ContextSummary, SanitizedEvent, SanitizedEventType, SourceTier,
    StructuredContext,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn default_config() -> BenchmarkRunConfig {
    BenchmarkRunConfig {
        inputs: vec![
            repo_root().join("data/traces/scenarios/morning-routine.jsonl"),
            repo_root().join("data/traces/scenarios/multi-app-switching.jsonl"),
            repo_root().join("data/traces/scenarios/rich-workflow.jsonl"),
        ],
        labels: repo_root().join("data/traces/synthetic-next-app-v1.labels.jsonl"),
        train_split: 0.7,
        window_secs: 10,
    }
}

#[test]
fn report_schema_and_counts_are_correct() {
    let report = run_benchmark(&default_config()).expect("benchmark should run");

    assert_eq!(report.schema_version, "dipecs.next_app_benchmark.v2");
    assert_eq!(report.dataset_id, "synthetic-next-app-v1");
    assert_eq!(report.total_windows, 946);
    assert_eq!(report.eligible_windows, 178);
    assert_eq!(report.scenarios.len(), 3);
    assert!(report.test_windows > 0);
    assert!(report.train_windows > 0);

    let expected_backends = [
        "rule_based",
        "local_evaluator",
        "always_noop",
        "random_candidate",
        "first_candidate",
        "global_majority",
        "per_current_app_majority",
        "markov",
        "recent_notification",
        "last_foreground",
        "notification_priority",
        "last_app_prewarm",
    ];
    for name in expected_backends {
        assert!(
            report.aggregate.contains_key(name),
            "missing aggregate backend {name}"
        );
    }

    for scenario in &report.scenarios {
        assert_eq!(scenario.eligible_windows, scenario.exclusions.eligible);
        assert_eq!(
            scenario.future_switch_windows,
            scenario.eligible_windows + scenario.exclusions.label_not_observable
        );
        for name in expected_backends {
            assert!(
                scenario.backends.contains_key(name),
                "missing backend {name} in scenario {}",
                scenario.scenario
            );
        }
    }
}

#[test]
fn rule_based_and_local_evaluator_metrics_are_internally_consistent() {
    let report = run_benchmark(&default_config()).expect("benchmark should run");

    for (name, metrics) in &report.aggregate {
        if name != "rule_based" && name != "local_evaluator" {
            continue;
        }
        let eligible = metrics.eligible_windows as f64;
        let predicted = metrics.predicted_windows as f64;
        let top1 = metrics.top1_hits as f64;
        let top3 = metrics.top3_hits as f64;

        assert!(metrics.predicted_windows <= metrics.eligible_windows);
        assert!(metrics.top1_hits <= metrics.predicted_windows);
        assert!(metrics.top3_hits >= metrics.top1_hits);

        let eps = 0.001;
        assert!((metrics.top1_accuracy_pct - top1 / eligible * 100.0).abs() < eps);
        assert!((metrics.top3_accuracy_pct - top3 / eligible * 100.0).abs() < eps);
        assert!((metrics.prediction_coverage_pct - predicted / eligible * 100.0).abs() < eps);
        assert!((metrics.conditional_top1_accuracy_pct - top1 / predicted * 100.0).abs() < eps);
        assert!(
            (metrics.wrong_prediction_rate_pct - (predicted - top1) / predicted * 100.0).abs()
                < eps
        );
        assert!(
            (metrics.no_prediction_rate_pct - (eligible - predicted) / eligible * 100.0).abs()
                < eps
        );
    }
}

#[test]
fn first_candidate_is_deterministic_and_picks_first_candidate() {
    let ctx = synthetic_context("A", &["B", "C"]);
    let backend = FirstCandidateBackend;
    let result = backend.predict(&ctx, "A", &["B".into(), "C".into()]);
    assert_eq!(result.ranked.len(), 1);
    assert_eq!(result.ranked[0].package, "B");
}

#[test]
fn random_candidate_is_deterministic_and_is_a_permutation() {
    let ctx = synthetic_context("A", &["B", "C", "D"]);
    let backend = RandomCandidateBackend::new(42);
    let r1 = backend.predict(&ctx, "A", &["B".into(), "C".into(), "D".into()]);
    let r2 = backend.predict(&ctx, "A", &["B".into(), "C".into(), "D".into()]);
    assert_eq!(r1.ranked, r2.ranked);
    let mut got: Vec<_> = r1.ranked.iter().map(|p| p.package.clone()).collect();
    got.sort();
    assert_eq!(got, vec!["B", "C", "D"]);
}

#[test]
fn markov_learns_transition_counts() {
    let train = vec![
        label("A", "B", true),
        label("A", "B", true),
        label("A", "C", true),
    ];
    let mut markov = MarkovBackend::default();
    markov.train(&train);

    let ctx = synthetic_context("A", &["B", "C"]);
    let result = markov.predict(&ctx, "A", &["B".into(), "C".into()]);
    assert_eq!(result.ranked[0].package, "B");
    assert!(result.ranked[0].score > result.ranked[1].score);
    assert!(result.ranked.iter().any(|p| p.package == "C"));
}

#[test]
fn per_current_app_majority_ignores_unseen_current_app() {
    let train = vec![label("A", "B", true), label("A", "B", true)];
    let mut backend = PerCurrentAppMajorityBackend::default();
    backend.train(&train);

    let ctx = synthetic_context("C", &["B", "D"]);
    let result = backend.predict(&ctx, "C", &["B".into(), "D".into()]);
    assert_eq!(result.ranked[0].package, "B");
    assert_eq!(result.ranked[1].package, "D");
    assert!(result.ranked.iter().all(|p| p.score == 0.0));
}

fn label(
    current_app: &str,
    actual_next_app: &str,
    eligible: bool,
) -> aios_cli::benchmark_next_app::types::NextAppLabel {
    aios_cli::benchmark_next_app::types::NextAppLabel {
        dataset_id: "test".into(),
        scenario: "test".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        prediction_horizon_ms: 30000,
        current_app: current_app.into(),
        observable_candidates: vec![],
        actual_next_app: Some(actual_next_app.into()),
        eligible,
        excluded_reason: None,
    }
}

fn synthetic_context(current_app: &str, candidates: &[&str]) -> StructuredContext {
    let mut events = vec![SanitizedEvent {
        event_id: "e1".into(),
        timestamp_ms: 500,
        event_type: SanitizedEventType::AppTransition {
            package_name: current_app.into(),
            activity_class: None,
            transition: AppTransition::Foreground,
        },
        source_tier: SourceTier::PublicApi,
        app_package: Some(current_app.into()),
        uid: None,
    }];
    for (i, c) in candidates.iter().enumerate() {
        events.push(SanitizedEvent {
            event_id: format!("n{i}"),
            timestamp_ms: 200 + i as i64,
            event_type: SanitizedEventType::Notification {
                source_package: (*c).into(),
                category: None,
                channel_id: None,
                title_hint: aios_spec::TextHint {
                    length_chars: 0,
                    script: aios_spec::ScriptHint::Unknown,
                    is_emoji_only: false,
                },
                text_hint: aios_spec::TextHint {
                    length_chars: 0,
                    script: aios_spec::ScriptHint::Unknown,
                    is_emoji_only: false,
                },
                semantic_hints: vec![],
                is_ongoing: false,
                group_key: None,
            },
            source_tier: SourceTier::PublicApi,
            app_package: Some((*c).into()),
            uid: None,
        });
    }
    StructuredContext {
        window_id: "w1".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events,
        summary: ContextSummary {
            foreground_apps: vec![current_app.into()],
            notified_apps: candidates.iter().map(|&c| c.into()).collect(),
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    }
}
