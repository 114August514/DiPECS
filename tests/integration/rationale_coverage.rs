//! rationale_tags 覆盖率 baseline。
//!
//! 统计 RuleBased / LocalEvaluator 产出的 intent 中，带有 rationale_tags 的窗口比例。
//! 统计基线（random / markov 等）不产出 DiPECS intents，rationale_coverage_pct 应为 0.0。

use aios_cli::benchmark_next_app::types::BenchmarkReport;

use crate::benchmark_cache::cached_report;

// Measured on synthetic-next-app-v1: rule_based / local_evaluator both emit
// rationale_tags on 100% of their windows. Thresholds are set just below that
// (aggregate >= 95%, per scenario >= 90%) to leave margin for trace drift while
// still guarding the "every DiPECS decision is explainable" property.
const AGG_RATIONALE_COV_MIN: f64 = 95.0;
const SCENARIO_RATIONALE_COV_MIN: f64 = 90.0;

const STATISTICAL_BACKENDS: &[&str] = &[
    "always_noop",
    "random_candidate",
    "first_candidate",
    "global_majority",
    "per_current_app_majority",
    "markov",
];

fn assert_rationale_coverage(report: &BenchmarkReport, backend_name: &str) {
    let metrics = report
        .aggregate
        .get(backend_name)
        .unwrap_or_else(|| panic!("{backend_name} must be present in aggregate"));

    println!(
        "\n=== rationale_coverage: {backend_name} rationale_coverage_pct = {:.1}% ===",
        metrics.rationale_coverage_pct
    );

    assert!(
        metrics.rationale_coverage_pct >= AGG_RATIONALE_COV_MIN,
        "{backend_name} rationale_coverage_pct should be >= {AGG_RATIONALE_COV_MIN}%, got {:.1}%",
        metrics.rationale_coverage_pct
    );

    let mut mismatches: Vec<String> = Vec::new();
    for scenario in &report.scenarios {
        let m = scenario.backends.get(backend_name).unwrap_or_else(|| {
            panic!(
                "{backend_name} must be present in scenario {}",
                scenario.scenario
            )
        });
        if m.rationale_coverage_pct < SCENARIO_RATIONALE_COV_MIN {
            mismatches.push(format!(
                "{backend_name} in {}: rationale_coverage={:.1}% below threshold {SCENARIO_RATIONALE_COV_MIN}%",
                scenario.scenario, m.rationale_coverage_pct
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{backend_name} scenario-level rationale coverage drifted:\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn rule_based_intents_have_rationale_tags() {
    assert_rationale_coverage(cached_report(), "rule_based");
}

#[test]
fn local_evaluator_intents_have_rationale_tags() {
    assert_rationale_coverage(cached_report(), "local_evaluator");
}

/// 统计基线不产出 DiPECS intents，rationale_coverage_pct 应为 0.0。
#[test]
fn statistical_baselines_have_zero_rationale_coverage() {
    let report = cached_report();

    println!("\n=== rationale_coverage: statistical baselines ===");
    for name in STATISTICAL_BACKENDS {
        let metrics = report
            .aggregate
            .get(*name)
            .unwrap_or_else(|| panic!("{name} must be present in aggregate"));

        println!(
            "  {name}: rationale_coverage_pct = {:.1}%",
            metrics.rationale_coverage_pct
        );

        assert_eq!(
            metrics.rationale_coverage_pct, 0.0,
            "{name} rationale_coverage_pct should be 0.0 (no DiPECS intents), got {:.1}%",
            metrics.rationale_coverage_pct
        );
    }
}
