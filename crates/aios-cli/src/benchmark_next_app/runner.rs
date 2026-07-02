//! Benchmark runner: train/test split, per-scenario evaluation, and report generation.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{bail, Context, Result};
use tracing::info;

use super::baselines::{
    AlwaysNoOpBackend, FirstCandidateBackend, GlobalMajorityBackend, LastAppPrewarmBackend,
    LastForegroundBackend, MarkovBackend, NotificationPriorityBackend,
    PerCurrentAppMajorityBackend, RandomCandidateBackend, RecentNotificationBackend,
};
use super::context_loader::{extract_observable_candidates, load_contexts_by_label, load_labels};
use super::metrics::{compute_backend_metrics, round3, PredictionRecord};
use super::predictor::{LocalEvaluatorNextAppBackend, RuleBasedNextAppBackend};
use super::report::SCHEMA_VERSION;
use super::types::{
    BackendMetrics, BenchmarkConfig, BenchmarkReport, ExclusionCounts, NextAppLabel,
    NextAppPredictor, ScenarioReport,
};

pub struct BenchmarkRunConfig {
    pub inputs: Vec<std::path::PathBuf>,
    pub labels: std::path::PathBuf,
    pub train_split: f64,
    pub window_secs: u64,
}

pub fn run_benchmark(config: &BenchmarkRunConfig) -> Result<BenchmarkReport> {
    if !(0.0..1.0).contains(&config.train_split) {
        bail!("train_split must be in (0, 1), got {}", config.train_split);
    }

    let labels = load_labels(&config.labels)
        .with_context(|| format!("loading labels {}", config.labels.display()))?;

    let mut by_scenario: HashMap<String, Vec<NextAppLabel>> = HashMap::new();
    let mut exclusion_counts = ExclusionCounts::default();
    for label in &labels {
        by_scenario
            .entry(label.scenario.clone())
            .or_default()
            .push(label.clone());
        if label.eligible {
            exclusion_counts.eligible += 1;
        } else if let Some(reason) = &label.excluded_reason {
            match reason.as_str() {
                "label_not_observable" => exclusion_counts.label_not_observable += 1,
                "no_future_switch" => exclusion_counts.no_future_switch += 1,
                _ => {},
            }
        }
    }

    let horizon_secs = labels
        .first()
        .map(|l| l.prediction_horizon_ms / 1000)
        .unwrap_or(30);

    let input_pairs: Vec<(String, &Path)> = config
        .inputs
        .iter()
        .map(|p| {
            let scenario = p
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
                .with_context(|| format!("invalid input path {}", p.display()))?;
            Ok((scenario, p.as_path()))
        })
        .collect::<Result<Vec<_>>>()?;

    let contexts = load_contexts_by_label(&input_pairs, &labels, config.window_secs)
        .context("loading contexts from traces")?;

    let mut scenarios = Vec::new();
    let mut aggregate_records: HashMap<String, Vec<PredictionRecord>> = HashMap::new();
    let mut aggregate_latencies: HashMap<String, Vec<u64>> = HashMap::new();

    for (scenario_name, trace_path) in &input_pairs {
        let scenario_labels = by_scenario
            .remove(scenario_name)
            .with_context(|| format!("no labels for scenario '{}'", scenario_name))?;

        let mut eligible: Vec<NextAppLabel> = scenario_labels
            .iter()
            .filter(|l| l.eligible)
            .cloned()
            .collect();
        eligible.sort_by_key(|l| l.window_start_ms);

        let n = eligible.len();
        let split_idx = ((n as f64) * config.train_split)
            .floor()
            .max(1.0)
            .min(n.saturating_sub(1) as f64) as usize;
        let (train, test) = eligible.split_at(split_idx);

        let mut predictors: Vec<Box<dyn NextAppPredictor>> = vec![
            Box::new(RuleBasedNextAppBackend),
            Box::new(LocalEvaluatorNextAppBackend),
            Box::new(AlwaysNoOpBackend),
            Box::new(RandomCandidateBackend::new(42)),
            Box::new(FirstCandidateBackend),
            Box::new(GlobalMajorityBackend::default()),
            Box::new(PerCurrentAppMajorityBackend::default()),
            Box::new(MarkovBackend::default()),
            Box::new(RecentNotificationBackend),
            Box::new(LastForegroundBackend),
            Box::new(NotificationPriorityBackend),
            Box::new(LastAppPrewarmBackend),
        ];

        for predictor in &mut predictors {
            predictor.train(train);
        }

        let mut backend_metrics: BTreeMap<String, BackendMetrics> = BTreeMap::new();
        for predictor in predictors.iter() {
            let (records, latencies) =
                evaluate_predictor(predictor.as_ref(), test, &contexts, scenario_name)?;
            let metrics = compute_backend_metrics(&records, test.len());
            backend_metrics.insert(predictor.name().to_string(), metrics);
            aggregate_records
                .entry(predictor.name().to_string())
                .or_default()
                .extend(records);
            aggregate_latencies
                .entry(predictor.name().to_string())
                .or_default()
                .extend(latencies);
        }

        let scenario_exclusions =
            exclusion_counts_for_scenario(&scenario_labels.iter().collect::<Vec<_>>());

        scenarios.push(ScenarioReport {
            scenario: (*scenario_name).to_string(),
            windows: scenario_labels.len(),
            future_switch_windows: scenario_labels.len() - scenario_exclusions.no_future_switch,
            eligible_windows: scenario_exclusions.eligible,
            train_windows: train.len(),
            test_windows: test.len(),
            exclusions: scenario_exclusions,
            backends: backend_metrics,
        });

        info!(
            scenario = %scenario_name,
            train = train.len(),
            test = test.len(),
            trace = %trace_path.display(),
            "benchmarked scenario"
        );
    }

    if !by_scenario.is_empty() {
        bail!(
            "missing input traces for scenarios: {:?}",
            by_scenario.keys()
        );
    }

    let mut aggregate: BTreeMap<String, BackendMetrics> = BTreeMap::new();
    for (name, records) in &aggregate_records {
        let mut metrics = compute_backend_metrics(records, records.len());
        let scenario_top1: Vec<f64> = scenarios
            .iter()
            .filter(|s| s.test_windows > 0)
            .filter_map(|s| s.backends.get(name).map(|m| m.top1_accuracy_pct))
            .collect();
        let scenario_top3: Vec<f64> = scenarios
            .iter()
            .filter(|s| s.test_windows > 0)
            .filter_map(|s| s.backends.get(name).map(|m| m.top3_accuracy_pct))
            .collect();
        metrics.macro_top1_accuracy_pct = Some(round3(
            scenario_top1.iter().sum::<f64>() / scenario_top1.len().max(1) as f64,
        ));
        metrics.macro_top3_accuracy_pct = Some(round3(
            scenario_top3.iter().sum::<f64>() / scenario_top3.len().max(1) as f64,
        ));
        if let Some(latencies) = aggregate_latencies.get(name) {
            metrics.latency_us = super::metrics::latency_summary(latencies);
        }
        aggregate.insert(name.clone(), metrics);
    }

    let future_switch_windows = exclusion_counts.eligible + exclusion_counts.label_not_observable;
    let coverage_pct =
        round3((exclusion_counts.eligible as f64 / future_switch_windows.max(1) as f64) * 100.0);

    Ok(BenchmarkReport {
        schema_version: SCHEMA_VERSION.into(),
        dataset_id: labels
            .first()
            .map(|l| l.dataset_id.clone())
            .unwrap_or_default(),
        source: "synthetic".into(),
        config: BenchmarkConfig {
            window_secs: config.window_secs,
            horizon_secs,
            label_definition: "first foreground transition within the horizon whose package differs from the app current at window end".into(),
            eligibility_definition: "actual_next_app is present in the current window's foreground or notified app set after removing current_app".into(),
            train_split: config.train_split,
        },
        inputs: config.inputs.clone(),
        total_windows: labels.len(),
        eligible_windows: exclusion_counts.eligible,
        train_windows: scenarios.iter().map(|s| s.train_windows).sum(),
        test_windows: scenarios.iter().map(|s| s.test_windows).sum(),
        context_supported_switch_coverage_pct: coverage_pct,
        exclusions: exclusion_counts,
        aggregate,
        scenarios,
        limitations: vec![
            "All labels are derived from deterministic synthetic traces, not real-user behavior.".into(),
            "Accuracy is reported only for context-supported switches; coverage reports how selective that cohort is.".into(),
            "Latency is measured during this run and is not byte-deterministic across machines.".into(),
            "Statistical baselines are trained on the per-scenario train split and evaluated on the held-out test split.".into(),
        ],
    })
}

fn evaluate_predictor(
    predictor: &dyn NextAppPredictor,
    test: &[NextAppLabel],
    contexts: &BTreeMap<(String, i64, i64), aios_spec::StructuredContext>,
    scenario: &str,
) -> Result<(Vec<PredictionRecord>, Vec<u64>)> {
    let mut records = Vec::with_capacity(test.len());
    let mut latencies = Vec::with_capacity(test.len());

    for label in test {
        let ctx = contexts
            .get(&(
                label.scenario.clone(),
                label.window_start_ms,
                label.window_end_ms,
            ))
            .with_context(|| {
                format!(
                    "missing context for scenario {} window {}-{}",
                    scenario, label.window_start_ms, label.window_end_ms
                )
            })?;
        let current_app = &label.current_app;
        let candidates = extract_observable_candidates(ctx, current_app);

        let result = predictor.predict(ctx, current_app, &candidates);
        let rank = if let Some(actual) = &label.actual_next_app {
            result
                .ranked
                .iter()
                .position(|p| &p.package == actual)
                .map(|i| i + 1)
        } else {
            None
        };

        records.push(PredictionRecord {
            rank,
            latency_us: result.latency_us,
            noop: result.ranked.is_empty(),
            rationale_present: result.rationale_present,
        });
        latencies.push(result.latency_us);
    }

    Ok((records, latencies))
}

fn exclusion_counts_for_scenario(labels: &[&NextAppLabel]) -> ExclusionCounts {
    let mut counts = ExclusionCounts::default();
    for label in labels {
        if label.eligible {
            counts.eligible += 1;
        } else if let Some(reason) = &label.excluded_reason {
            match reason.as_str() {
                "label_not_observable" => counts.label_not_observable += 1,
                "no_future_switch" => counts.no_future_switch += 1,
                _ => {},
            }
        }
    }
    counts
}
