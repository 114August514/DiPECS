//! rationale_tags 覆盖率 baseline。
//!
//! 统计 RuleBased / LocalEvaluator 产出的 intent 中，带有 rationale_tags 的窗口比例。
//! 统计基线（random / markov 等）不产出 DiPECS intents，rationale_coverage_pct 应为 0.0。

use crate::benchmark_cache::cached_report;

/// RuleBased 在每个窗口产出的 intent 中 rationale_tags 非空的比例 > 50%。
///
/// RuleBasedBackend 对所有已识别的事件类型（notification、foreground、screen_on 等）
/// 都会附带 rationale_tags，因此覆盖率应接近 100%；保守断言 > 50%。
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
        metrics.rationale_coverage_pct > 50.0,
        "rule_based rationale_coverage_pct should be > 50%, got {:.1}%",
        metrics.rationale_coverage_pct
    );
}

/// LocalEvaluator 同样应当 > 50%。
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
        metrics.rationale_coverage_pct > 50.0,
        "local_evaluator rationale_coverage_pct should be > 50%, got {:.1}%",
        metrics.rationale_coverage_pct
    );
}

/// 统计基线（random / first / global_majority / per_current_app_majority / markov / always_noop）
/// 不产出 DiPECS intents，rationale_coverage_pct 应为 0.0。
#[test]
fn statistical_baselines_have_zero_rationale_coverage() {
    let report = cached_report();

    let statistical_backends = [
        "always_noop",
        "random_candidate",
        "first_candidate",
        "global_majority",
        "per_current_app_majority",
        "markov",
    ];

    println!("\n=== rationale_coverage: statistical baselines ===");
    for name in statistical_backends {
        let metrics = report
            .aggregate
            .get(name)
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
