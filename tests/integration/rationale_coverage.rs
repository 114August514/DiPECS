//! rationale_tags 覆盖率 baseline。
//!
//! 统计 RuleBased / LocalEvaluator 产出的 intent 中，带有 rationale_tags 的窗口比例。
//! 统计基线（random / markov 等）不产出 DiPECS intents，rationale_coverage_pct 应为 0.0。

use crate::benchmark_cache::cached_report;

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

#[test]
fn rule_based_intents_have_rationale_tags() {
    let report = cached_report();

    let metrics = report
        .aggregate
        .get("rule_based")
        .expect("rule_based must be present in aggregate");

    println!(
        "\n=== rationale_coverage: rule_based rationale_coverage_pct = {:.1}% ===",
        metrics.rationale_coverage_pct
    );

    assert!(
        metrics.rationale_coverage_pct >= AGG_RATIONALE_COV_MIN,
        "rule_based rationale_coverage_pct should be >= {AGG_RATIONALE_COV_MIN}%, got {:.1}%",
        metrics.rationale_coverage_pct
    );

    let mut mismatches: Vec<String> = Vec::new();
    for scenario in &report.scenarios {
        let m = scenario.backends.get("rule_based").unwrap_or_else(|| {
            panic!(
                "rule_based must be present in scenario {}",
                scenario.scenario
            )
        });
        if m.rationale_coverage_pct < SCENARIO_RATIONALE_COV_MIN {
            mismatches.push(format!(
                "rule_based in {}: rationale_coverage={:.1}% below threshold {SCENARIO_RATIONALE_COV_MIN}%",
                scenario.scenario, m.rationale_coverage_pct
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "rule_based scenario-level rationale coverage drifted:\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn local_evaluator_intents_have_rationale_tags() {
    let report = cached_report();

    let metrics = report
        .aggregate
        .get("local_evaluator")
        .expect("local_evaluator must be present in aggregate");

    println!(
        "\n=== rationale_coverage: local_evaluator rationale_coverage_pct = {:.1}% ===",
        metrics.rationale_coverage_pct
    );

    assert!(
        metrics.rationale_coverage_pct >= AGG_RATIONALE_COV_MIN,
        "local_evaluator rationale_coverage_pct should be >= {AGG_RATIONALE_COV_MIN}%, got {:.1}%",
        metrics.rationale_coverage_pct
    );

    let mut mismatches: Vec<String> = Vec::new();
    for scenario in &report.scenarios {
        let m = scenario.backends.get("local_evaluator").unwrap_or_else(|| {
            panic!(
                "local_evaluator must be present in scenario {}",
                scenario.scenario
            )
        });
        if m.rationale_coverage_pct < SCENARIO_RATIONALE_COV_MIN {
            mismatches.push(format!(
                "local_evaluator in {}: rationale_coverage={:.1}% below threshold {SCENARIO_RATIONALE_COV_MIN}%",
                scenario.scenario, m.rationale_coverage_pct
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "local_evaluator scenario-level rationale coverage drifted:\n{}",
        mismatches.join("\n")
    );
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
