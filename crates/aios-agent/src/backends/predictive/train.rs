//! Offline training for the next-app predictive model.

use std::collections::{BTreeMap, BTreeSet};

use super::ensemble::fit_ensemble_models;
use super::predictor::feature_keys;
use super::{
    prediction_features_for_example, score_order, AppScore, EnsembleCombiner, FeatureLiftModel,
    FeatureLiftTree, LogisticRerankerModel, MarkovModel, NaiveBayesModel, NextAppModelArtifact,
    NextAppModelConfig, NextAppTrainingExample, TrainingSummary, MAX_TRAINED_FEATURES, MODEL_NAME,
    SCHEMA_VERSION,
};

pub fn train_next_app_artifact(
    dataset_id: impl Into<String>,
    config: NextAppModelConfig,
    examples: &[NextAppTrainingExample],
) -> Result<NextAppModelArtifact, String> {
    let dataset_id = dataset_id.into();
    // Train the base model (Naive Bayes, Markov, feature-lift) on all examples,
    // then fit the learned ensemble combiner on a held-out validation slice.
    let mut artifact = train_base_artifact(&dataset_id, config.clone(), examples)?;
    let ensemble_models = fit_ensemble_models(&dataset_id, &config, examples);
    artifact.ensemble_combiner = ensemble_models.rrf;
    artifact.ensemble_logistic = ensemble_models.logistic;
    Ok(artifact)
}

/// Train only the base component models (no ensemble combiner fit). Used both
/// for the final artifact's base and for the combiner's fit-slice model, so the
/// combiner fit never recurses into itself.
pub(super) fn train_base_artifact(
    dataset_id: &str,
    config: NextAppModelConfig,
    examples: &[NextAppTrainingExample],
) -> Result<NextAppModelArtifact, String> {
    if examples.is_empty() {
        return Err("cannot train next-app model with zero examples".into());
    }

    let mut app_set = BTreeSet::new();
    let mut user_set = BTreeSet::new();
    for example in examples {
        app_set.insert(example.current_app.clone());
        app_set.insert(example.label_app.clone());
        user_set.insert(example.user_id.clone());
    }
    let app_vocab: Vec<String> = app_set.into_iter().collect();
    let app_index: BTreeMap<String, usize> = app_vocab
        .iter()
        .enumerate()
        .map(|(i, app)| (app.clone(), i))
        .collect();
    let classes = app_vocab.len();

    let mut class_counts = vec![0_u32; classes];
    let mut feature_counts: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut feature_frequency: BTreeMap<String, u32> = BTreeMap::new();
    let mut global_transitions: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    let mut user_transitions: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    let mut global_transitions_order2: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    // Temporal Markov transitions: keyed by "{current}\t{hour}" or
    // "{current}\t{weekday}" for context-aware next-app prediction.
    let mut temporal_transitions: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    // Per-user app-usage frequency (MFU): counts each observed next app per
    // user, unconditional on the current app.
    let mut user_frequency_counts: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    // Hard recency pointer: last observed next app per (user, current), in
    // example order (last write wins). Examples are chronological per user.
    let mut user_recency: BTreeMap<String, String> = BTreeMap::new();

    for example in examples {
        let label_idx = *app_index
            .get(&example.label_app)
            .ok_or_else(|| format!("label not in vocab: {}", example.label_app))?;
        class_counts[label_idx] += 1;

        *user_frequency_counts
            .entry(example.user_id.clone())
            .or_default()
            .entry(example.label_app.clone())
            .or_default() += 1;

        user_recency.insert(
            user_transition_key(&example.user_id, &example.current_app),
            example.label_app.clone(),
        );

        let features = training_features(example);
        for feature in features {
            let counts = feature_counts
                .entry(feature.clone())
                .or_insert(vec![0; classes]);
            counts[label_idx] += 1;
            *feature_frequency.entry(feature).or_default() += 1;
        }

        *global_transitions
            .entry(example.current_app.clone())
            .or_default()
            .entry(example.label_app.clone())
            .or_default() += 1;
        *user_transitions
            .entry(user_transition_key(&example.user_id, &example.current_app))
            .or_default()
            .entry(example.label_app.clone())
            .or_default() += 1;

        // Order-2 transitions keyed (prev, current). The "previous app" is the
        // most recent history entry that is not the current app itself, so that
        // repeated current-app foreground records do not collapse the key into
        // (current, current). This matches how the strong baseline derives its
        // order-2 previous app and how `prev=` is intended to behave.
        if let Some(prev) = previous_app(example) {
            *global_transitions_order2
                .entry(order2_key(prev, &example.current_app))
                .or_default()
                .entry(example.label_app.clone())
                .or_default() += 1;
        }

        // Temporal transitions keyed by hour and weekday.
        let hour_key = format!("{}\t{}", example.current_app, example.hour_bucket);
        *temporal_transitions
            .entry(hour_key)
            .or_default()
            .entry(example.label_app.clone())
            .or_default() += 1;
        let weekday_key = format!("{}\t{}", example.current_app, example.weekday);
        *temporal_transitions
            .entry(weekday_key)
            .or_default()
            .entry(example.label_app.clone())
            .or_default() += 1;
    }

    let global_popularity = counts_to_scores(&class_counts, &app_vocab);
    let total_examples = examples.len() as f32;
    let class_log_priors: Vec<f32> = class_counts
        .iter()
        .map(|count| ((*count as f32 + 1.0) / (total_examples + classes as f32)).ln())
        .collect();
    let unknown_feature_log_probs = class_counts
        .iter()
        .map(|count| (1.0 / (*count as f32 + 2.0)).ln())
        .collect();
    let feature_log_probs = feature_counts
        .iter()
        .map(|(feature, counts)| {
            let probs = counts
                .iter()
                .enumerate()
                .map(|(idx, count)| ((*count as f32 + 1.0) / (class_counts[idx] as f32 + 2.0)).ln())
                .collect();
            (feature.clone(), probs)
        })
        .collect();

    let mut feature_order: Vec<(String, u32)> = feature_frequency.into_iter().collect();
    feature_order.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let trees = feature_order
        .into_iter()
        .take(MAX_TRAINED_FEATURES)
        .filter_map(|(feature, _)| {
            feature_counts.get(&feature).map(|counts| FeatureLiftTree {
                feature_key: feature,
                yes_scores: counts_to_log_lift_scores(counts, &class_counts, &app_vocab),
            })
        })
        .collect();

    Ok(NextAppModelArtifact {
        schema_version: SCHEMA_VERSION.into(),
        model_id: MODEL_NAME.into(),
        dataset_id: dataset_id.to_string(),
        trained_at_ms: 0,
        config,
        app_vocab,
        global_popularity,
        naive_bayes: NaiveBayesModel {
            class_log_priors: class_log_priors.clone(),
            unknown_feature_log_probs,
            feature_log_probs,
        },
        markov: MarkovModel {
            global_transitions: transition_scores(global_transitions),
            user_transitions: transition_scores(user_transitions),
            global_transitions_order2: transition_scores(global_transitions_order2),
        },
        feature_lift: FeatureLiftModel {
            base_scores: class_log_priors,
            trees,
        },
        user_frequency: transition_scores(user_frequency_counts),
        user_recency,
        markov_context: transition_scores(temporal_transitions),
        ensemble_combiner: EnsembleCombiner::default(),
        ensemble_logistic: LogisticRerankerModel::default(),
        training_summary: TrainingSummary {
            examples: examples.len(),
            users: user_set.len(),
            apps: classes,
        },
    })
}

fn training_features(example: &NextAppTrainingExample) -> Vec<String> {
    feature_keys(&prediction_features_for_example(example))
}

fn counts_to_scores(counts: &[u32], app_vocab: &[String]) -> Vec<AppScore> {
    let total: u32 = counts.iter().sum();
    let denom = total as f32 + app_vocab.len() as f32;
    let mut scores: Vec<AppScore> = app_vocab
        .iter()
        .cloned()
        .zip(counts.iter())
        .map(|(app, count)| AppScore {
            app,
            score: (*count as f32 + 1.0) / denom,
        })
        .collect();
    scores.sort_by(|a, b| score_order(a.score, b.score).then_with(|| a.app.cmp(&b.app)));
    scores
}

fn counts_to_log_lift_scores(
    feature_counts: &[u32],
    class_counts: &[u32],
    app_vocab: &[String],
) -> Vec<AppScore> {
    let feature_total: u32 = feature_counts.iter().sum();
    let class_total: u32 = class_counts.iter().sum();
    let class_len = app_vocab.len() as f32;
    let mut scores: Vec<AppScore> = app_vocab
        .iter()
        .enumerate()
        .map(|(idx, app)| {
            let p_label_given_feature =
                (feature_counts[idx] as f32 + 1.0) / (feature_total as f32 + class_len);
            let p_label = (class_counts[idx] as f32 + 1.0) / (class_total as f32 + class_len);
            AppScore {
                app: app.clone(),
                score: (p_label_given_feature / p_label).ln(),
            }
        })
        .collect();
    scores.sort_by(|a, b| score_order(a.score, b.score).then_with(|| a.app.cmp(&b.app)));
    scores
}

fn transition_scores(
    transitions: BTreeMap<String, BTreeMap<String, u32>>,
) -> BTreeMap<String, Vec<AppScore>> {
    transitions
        .into_iter()
        .map(|(from, counts)| {
            let total: u32 = counts.values().sum();
            let mut scores: Vec<AppScore> = counts
                .into_iter()
                .map(|(app, count)| AppScore {
                    app,
                    score: count as f32 / total.max(1) as f32,
                })
                .collect();
            scores.sort_by(|a, b| score_order(a.score, b.score).then_with(|| a.app.cmp(&b.app)));
            (from, scores)
        })
        .collect()
}

pub(crate) fn user_transition_key(user_id: &str, current_app: &str) -> String {
    format!("{user_id}\t{current_app}")
}

pub(crate) fn order2_key(prev_app: &str, current_app: &str) -> String {
    format!("{prev_app}\t{current_app}")
}

/// The most recent history entry that is not the current app itself. Skipping
/// repeated current-app records prevents the order-2 key from collapsing into
/// `(current, current)`.
pub(crate) fn previous_app(example: &NextAppTrainingExample) -> Option<&str> {
    example
        .history
        .iter()
        .rev()
        .find(|app| app.as_str() != example.current_app)
        .map(String::as_str)
}
