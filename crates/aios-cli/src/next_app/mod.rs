//! Offline next-app training/evaluation helpers for LSApp-shaped datasets.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;

use aios_agent::{
    prediction_features_for_example, train_next_app_artifact, NextAppAlgorithm,
    NextAppModelArtifact, NextAppModelConfig, NextAppPredictor,
};
use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::Serialize;

mod adaptive_baseline;
mod baselines;
mod loader;
mod metrics;
mod net_benefit;
mod split;
mod strong_baseline;

pub use net_benefit::{compute_net_benefit, NetBenefitInputs, NetBenefitReport};

use adaptive_baseline::AdaptiveBaseline;
use baselines::BaselineTables;
use loader::load_examples;
use metrics::evaluate_ranker;
use split::split_examples;
use strong_baseline::StrongPredictiveActionBaseline;

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum NextAppDataset {
    Lsapp,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum NextAppSplit {
    Standard,
    ColdStart,
}

#[derive(Debug, Clone)]
pub struct TrainOptions {
    pub dataset: NextAppDataset,
    pub input: PathBuf,
    pub output: PathBuf,
    pub horizon_secs: u64,
    pub history_len: usize,
    pub split: NextAppSplit,
}

#[derive(Debug, Clone)]
pub struct EvalOptions {
    pub dataset: NextAppDataset,
    pub input: PathBuf,
    pub artifact: PathBuf,
    pub output: PathBuf,
    pub horizon_secs: u64,
    pub history_len: usize,
    pub split: NextAppSplit,
}

#[derive(Debug, Serialize)]
struct EvalReport {
    schema_version: String,
    dataset: String,
    split: String,
    artifact_model_id: String,
    artifact_dataset_id: String,
    total_examples: usize,
    train_examples: usize,
    test_examples: usize,
    metrics: BTreeMap<String, MetricsReport>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct MetricsReport {
    pub examples: usize,
    pub predicted: usize,
    pub hit_rate_at_1_pct: f32,
    pub hit_rate_at_3_pct: f32,
    pub hit_rate_at_5_pct: f32,
    pub mean_reciprocal_rank_at_5: f32,
    pub prediction_coverage_pct: f32,
    pub macro_hit_rate_at_1_pct: f32,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct UserAccum {
    pub examples: usize,
    pub hit1: usize,
}

pub fn train(opts: TrainOptions) -> Result<()> {
    let examples = load_examples(&opts.input, opts.horizon_secs, opts.history_len)
        .with_context(|| format!("loading LSApp dataset from {}", opts.input.display()))?;
    let (train_examples, _) = split_examples(&examples, opts.split);
    let config = NextAppModelConfig {
        horizon_secs: opts.horizon_secs,
        history_len: opts.history_len,
    };
    let artifact =
        train_next_app_artifact("lsapp", config, &train_examples).map_err(anyhow::Error::msg)?;

    if let Some(parent) = opts.output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating output dir {}", parent.display()))?;
        }
    }
    let file = File::create(&opts.output)
        .with_context(|| format!("creating artifact {}", opts.output.display()))?;
    serde_json::to_writer_pretty(BufWriter::new(file), &artifact)
        .with_context(|| format!("writing artifact {}", opts.output.display()))?;
    eprintln!(
        "trained next-app artifact: {} examples, {} apps -> {}",
        artifact.training_summary.examples,
        artifact.training_summary.apps,
        opts.output.display()
    );
    Ok(())
}

pub fn evaluate(opts: EvalOptions) -> Result<()> {
    let artifact_file = File::open(&opts.artifact)
        .with_context(|| format!("opening artifact {}", opts.artifact.display()))?;
    let artifact: NextAppModelArtifact = serde_json::from_reader(BufReader::new(artifact_file))
        .with_context(|| format!("parsing artifact {}", opts.artifact.display()))?;
    let predictor = NextAppPredictor::new(artifact.clone()).map_err(anyhow::Error::msg)?;

    let examples = load_examples(&opts.input, opts.horizon_secs, opts.history_len)
        .with_context(|| format!("loading LSApp dataset from {}", opts.input.display()))?;
    let (train_examples, test_examples) = split_examples(&examples, opts.split);
    if test_examples.is_empty() {
        bail!("split produced zero test examples");
    }
    let baseline = BaselineTables::from_training(&train_examples);
    let strong_baseline = StrongPredictiveActionBaseline::from_training(&train_examples);

    let mut metrics = BTreeMap::new();
    metrics.insert(
        "global_popularity".into(),
        evaluate_ranker(&test_examples, |_example| {
            baseline.global_popularity.iter().take(5).cloned().collect()
        }),
    );
    metrics.insert(
        "mfu".into(),
        evaluate_ranker(&test_examples, |example| baseline.mfu(&example.user_id)),
    );
    metrics.insert(
        "mru".into(),
        evaluate_ranker(&test_examples, |example| {
            example
                .history
                .last()
                .cloned()
                .map(|app| vec![app])
                .unwrap_or_default()
        }),
    );
    for (name, algorithm) in [
        ("naive_bayes", NextAppAlgorithm::NaiveBayes),
        ("markov", NextAppAlgorithm::Markov),
        // The report key remains "xgboost" for backward compatibility with
        // committed baseline reports; the underlying model is a log-lift
        // feature ensemble, not XGBoost.
        ("xgboost", NextAppAlgorithm::FeatureLift),
        ("ensemble", NextAppAlgorithm::Ensemble),
    ] {
        metrics.insert(
            name.into(),
            evaluate_ranker(&test_examples, |example| {
                predictor
                    .rank(&prediction_features_for_example(example), algorithm, 5)
                    .into_iter()
                    .map(|score| score.app)
                    .collect()
            }),
        );
    }
    metrics.insert(
        "strong_predictive".into(),
        evaluate_ranker(&test_examples, |example| {
            strong_baseline.predict_for_example(example, 5)
        }),
    );
    let adaptive_baseline = AdaptiveBaseline::from_training(&train_examples);
    metrics.insert(
        "adaptive_predictive".into(),
        evaluate_ranker(&test_examples, |example| {
            adaptive_baseline.predict_for_example(example, 5)
        }),
    );

    let report = EvalReport {
        schema_version: "dipecs.next_app_eval.v1".into(),
        dataset: format!("{:?}", opts.dataset),
        split: format!("{:?}", opts.split),
        artifact_model_id: artifact.model_id,
        artifact_dataset_id: artifact.dataset_id,
        total_examples: examples.len(),
        train_examples: train_examples.len(),
        test_examples: test_examples.len(),
        metrics,
    };

    if let Some(parent) = opts.output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating output dir {}", parent.display()))?;
        }
    }
    let file = File::create(&opts.output)
        .with_context(|| format!("creating report {}", opts.output.display()))?;
    serde_json::to_writer_pretty(BufWriter::new(file), &report)
        .with_context(|| format!("writing report {}", opts.output.display()))?;
    eprintln!(
        "evaluated next-app artifact on {} test examples -> {}",
        report.test_examples,
        opts.output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use aios_agent::NextAppTrainingExample;

    use super::*;

    #[test]
    fn train_and_eval_lsapp_fixture() {
        let dir = unique_tmp_dir();
        fs::create_dir_all(&dir).expect("create tmp dir");
        let input = dir.join("lsapp.csv");
        let artifact = dir.join("artifact.json");
        let report = dir.join("report.json");
        write_fixture(&input);

        train(TrainOptions {
            dataset: NextAppDataset::Lsapp,
            input: input.clone(),
            output: artifact.clone(),
            horizon_secs: 30,
            history_len: 3,
            split: NextAppSplit::Standard,
        })
        .expect("train should succeed");
        evaluate(EvalOptions {
            dataset: NextAppDataset::Lsapp,
            input,
            artifact,
            output: report.clone(),
            horizon_secs: 30,
            history_len: 3,
            split: NextAppSplit::Standard,
        })
        .expect("eval should succeed");

        let value: serde_json::Value =
            serde_json::from_reader(File::open(report).expect("report should exist"))
                .expect("report should parse");
        assert_eq!(value["schema_version"], "dipecs.next_app_eval.v1");
        assert!(value["metrics"]["ensemble"]["examples"].as_u64().unwrap() > 0);
    }

    #[test]
    fn lsapp_loader_orders_sessions_chronologically_not_lexically() {
        let dir = unique_tmp_dir();
        fs::create_dir_all(&dir).expect("create tmp dir");
        let input = dir.join("lsapp.csv");
        let mut file = fs::File::create(&input).unwrap();
        writeln!(file, "user_id,session_id,timestamp_ms,app_name,event_type").unwrap();
        writeln!(file, "u1,10,3000,com.late,foreground").unwrap();
        writeln!(file, "u1,10,4000,com.final,foreground").unwrap();
        writeln!(file, "u1,2,1000,com.early,foreground").unwrap();
        writeln!(file, "u1,2,2000,com.shared,foreground").unwrap();

        let examples = super::loader::load_examples(&input, 30, 4).expect("fixture should load");

        let late = examples
            .iter()
            .find(|example| example.current_app == "com.late")
            .expect("late session should produce an example");
        assert_eq!(
            late.history,
            vec!["com.early".to_string(), "com.shared".to_string()],
            "numeric-looking session ids must not reorder later sessions before earlier history"
        );
    }

    #[test]
    fn explicit_timestamp_ms_parse_errors_are_rejected() {
        let dir = unique_tmp_dir();
        fs::create_dir_all(&dir).expect("create tmp dir");
        let input = dir.join("bad-timestamp.csv");
        let mut file = fs::File::create(&input).unwrap();
        writeln!(file, "user_id,session_id,timestamp_ms,app_name,event_type").unwrap();
        writeln!(file, "u1,s1,not-a-time,com.chat,foreground").unwrap();
        writeln!(file, "u1,s1,2000,com.mail,foreground").unwrap();

        let err = super::loader::load_examples(&input, 30, 3)
            .expect_err("malformed explicit timestamp_ms must not fall back to ordinal time");
        assert!(
            err.to_string().contains("timestamp_ms"),
            "error should name the malformed timestamp_ms column: {err:#}"
        );
    }

    #[test]
    fn directory_loader_ignores_plain_json_files() {
        let dir = unique_tmp_dir();
        fs::create_dir_all(&dir).expect("create tmp dir");
        let csv = dir.join("lsapp.csv");
        write_fixture(&csv);
        let mut json = fs::File::create(dir.join("notes.json")).unwrap();
        writeln!(json, r#"{{"not":"a supported LSApp record"}}"#).unwrap();
        writeln!(json, r#"{{"still":"not delimited"}}"#).unwrap();

        let examples = super::loader::load_examples(&dir, 30, 3)
            .expect("unsupported .json sidecars should be ignored");
        assert!(!examples.is_empty());
    }

    #[test]
    fn cold_start_split_uses_stable_hash_order_not_user_id_sort() {
        let examples: Vec<NextAppTrainingExample> = ["u0", "u1", "u2", "u3", "u4"]
            .into_iter()
            .map(|user| NextAppTrainingExample {
                user_id: user.into(),
                current_app: "com.chat".into(),
                history: vec![],
                hour_bucket: 9,
                weekday: 1,
                event_type: "foreground".into(),
                label_app: "com.mail".into(),
            })
            .collect();
        let (train, test) = split_examples(&examples, NextAppSplit::ColdStart);
        assert_eq!(train.len(), 4);
        assert_eq!(test.len(), 1);
        // Seeded FNV hash order for these user ids places u3 in test.
        assert_eq!(test[0].user_id, "u3");
    }

    fn write_fixture(path: &PathBuf) {
        let mut file = fs::File::create(path).unwrap();
        writeln!(file, "user_id,session_id,timestamp,app_name,event_type").unwrap();
        let mut ts = 1_700_000_000_000_i64;
        for user in ["u1", "u2"] {
            for session in ["s1", "s2"] {
                for app in [
                    "com.chat",
                    "com.mail",
                    "com.chat",
                    "com.mail",
                    "com.browser",
                ] {
                    writeln!(file, "{user},{session},{ts},{app},foreground").unwrap();
                    ts += 5_000;
                }
                ts += 60_000;
            }
        }
    }

    fn unique_tmp_dir() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("dipecs-next-app-test-{suffix}"))
    }

    #[test]
    fn evaluate_ranker_counts_non_empty_wrong_predictions_as_predicted() {
        let examples = vec![
            NextAppTrainingExample {
                user_id: "u1".into(),
                current_app: "com.chat".into(),
                history: vec![],
                hour_bucket: 9,
                weekday: 1,
                event_type: "foreground".into(),
                label_app: "com.mail".into(),
            },
            NextAppTrainingExample {
                user_id: "u1".into(),
                current_app: "com.chat".into(),
                history: vec![],
                hour_bucket: 9,
                weekday: 1,
                event_type: "foreground".into(),
                label_app: "com.browser".into(),
            },
        ];
        let report = evaluate_ranker(&examples, |_example| {
            vec!["com.other".into(), "com.yetanother".into()]
        });
        assert_eq!(report.examples, 2);
        assert_eq!(
            report.predicted, 2,
            "non-empty predictions must count toward coverage"
        );
        assert_eq!(report.hit_rate_at_1_pct, 0.0);
        assert_eq!(report.hit_rate_at_3_pct, 0.0);
        assert_eq!(report.hit_rate_at_5_pct, 0.0);
        assert_eq!(report.prediction_coverage_pct, 100.0);
    }
}
