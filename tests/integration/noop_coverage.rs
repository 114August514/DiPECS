//! NoOp 覆盖率 baseline：当前后端 vs 总是 NoOp 的简单策略。
//!
//! 复用 next-app benchmark 的跑分逻辑，统计 RuleBased / LocalEvaluator
//! 在每个场景下产出空预测（即 NoOp）的比例，并与 always_noop 的 100%
//! 做对比，量化真实动作覆盖率。

use crate::benchmark_cache::cached_report;

/// NoOp 率上限：真实后端必须显著低于 always-noop 的 100%。
const MAX_NOOP_RATE_PCT: f64 = 70.0;
/// 预测覆盖率下限：真实后端必须在相当比例的窗口上做出非 NoOp 预测。
const MIN_PREDICTION_COVERAGE_PCT: f64 = 30.0;

#[test]
fn rule_based_and_local_evaluator_noop_rate_is_better_than_always_noop() {
    let report = cached_report();

    let mut mismatches: Vec<String> = Vec::new();

    for scenario in &report.scenarios {
        if scenario.test_windows == 0 {
            continue;
        }

        for name in ["rule_based", "local_evaluator"] {
            let metrics = scenario.backends.get(name).unwrap_or_else(|| {
                panic!("missing backend {name} in scenario {}", scenario.scenario)
            });

            if metrics.noop_rate_pct >= 100.0 {
                mismatches.push(format!(
                    "{} in {}: noop_rate={:.3}% (not better than always NoOp)",
                    name, scenario.scenario, metrics.noop_rate_pct
                ));
            }
            if metrics.noop_rate_pct > MAX_NOOP_RATE_PCT {
                mismatches.push(format!(
                    "{} in {}: noop_rate={:.3}% exceeds threshold {MAX_NOOP_RATE_PCT}%",
                    name, scenario.scenario, metrics.noop_rate_pct
                ));
            }
            if metrics.prediction_coverage_pct < MIN_PREDICTION_COVERAGE_PCT {
                mismatches.push(format!(
                    "{} in {}: prediction_coverage={:.3}% below threshold {MIN_PREDICTION_COVERAGE_PCT}%",
                    name, scenario.scenario, metrics.prediction_coverage_pct
                ));
            }
        }

        // Sanity check: the always-noop baseline must be 100% NoOp.
        let always = scenario
            .backends
            .get("always_noop")
            .expect("always_noop backend must be present");
        assert!(
            (always.noop_rate_pct - 100.0).abs() < f64::EPSILON,
            "always_noop must have 100% noop_rate, got {:.3}% in {}",
            always.noop_rate_pct,
            scenario.scenario
        );
    }

    // Aggregate sanity: both real backends must beat always-noop overall.
    for name in ["rule_based", "local_evaluator"] {
        let metrics = report
            .aggregate
            .get(name)
            .unwrap_or_else(|| panic!("missing aggregate backend {name}"));
        assert!(
            metrics.noop_rate_pct < 100.0,
            "aggregate {name} noop_rate={:.3}% must be below 100%",
            metrics.noop_rate_pct
        );
        assert!(
            metrics.noop_rate_pct <= MAX_NOOP_RATE_PCT,
            "aggregate {name} noop_rate={:.3}% exceeds {MAX_NOOP_RATE_PCT}%",
            metrics.noop_rate_pct
        );
        assert!(
            metrics.prediction_coverage_pct >= MIN_PREDICTION_COVERAGE_PCT,
            "aggregate {name} prediction_coverage={:.3}% below {MIN_PREDICTION_COVERAGE_PCT}%",
            metrics.prediction_coverage_pct
        );
    }

    assert!(
        mismatches.is_empty(),
        "NoOp coverage baseline drifted:\n{}",
        mismatches.join("\n")
    );
}
