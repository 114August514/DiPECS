//! PredictiveLocalBackend - deterministic next-app prediction from exported artifacts.
//!
//! Training can happen offline, but runtime inference stays local and pure: the
//! backend reads a JSON artifact containing Naive Bayes, Markov, and exported
//! tree-ensemble scores, then emits low-risk app intent candidates.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use aios_spec::{
    ActionType, ActionUrgency, AppTransition, DecisionBackendResult, DecisionRoute, Intent,
    IntentBatch, IntentType, ModelInput, RiskLevel, SanitizedEventType, StructuredContext,
    SuggestedAction,
};
use serde::{Deserialize, Serialize};

use crate::{new_id, DecisionBackend};

const SCHEMA_VERSION: &str = "dipecs.next_app_model.v1";
const MODEL_NAME: &str = "predictive-local-v0.1";
const MAX_BACKEND_INTENTS: usize = 5;
const MAX_TRAINED_FEATURES: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextAppModelArtifact {
    pub schema_version: String,
    pub model_id: String,
    pub dataset_id: String,
    pub trained_at_ms: i64,
    pub config: NextAppModelConfig,
    pub app_vocab: Vec<String>,
    pub global_popularity: Vec<AppScore>,
    pub naive_bayes: NaiveBayesModel,
    pub markov: MarkovModel,
    pub xgboost: XgboostModel,
    pub training_summary: TrainingSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextAppModelConfig {
    pub horizon_secs: u64,
    pub history_len: usize,
}

impl Default for NextAppModelConfig {
    fn default() -> Self {
        Self {
            horizon_secs: 30,
            history_len: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingSummary {
    pub examples: usize,
    pub users: usize,
    pub apps: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppScore {
    pub app: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NaiveBayesModel {
    pub class_log_priors: Vec<f32>,
    pub unknown_feature_log_probs: Vec<f32>,
    pub feature_log_probs: BTreeMap<String, Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkovModel {
    pub global_transitions: BTreeMap<String, Vec<AppScore>>,
    pub user_transitions: BTreeMap<String, Vec<AppScore>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XgboostModel {
    pub base_scores: Vec<f32>,
    pub trees: Vec<XgboostTree>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XgboostTree {
    pub feature_key: String,
    pub yes_scores: Vec<AppScore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextAppAlgorithm {
    NaiveBayes,
    Markov,
    Xgboost,
    Ensemble,
}

#[derive(Debug, Clone)]
pub struct NextAppTrainingExample {
    pub user_id: String,
    pub current_app: String,
    pub history: Vec<String>,
    pub hour_bucket: u8,
    pub weekday: u8,
    pub event_type: String,
    pub label_app: String,
}

#[derive(Debug, Clone, Default)]
pub struct PredictionFeatures {
    pub user_id: Option<String>,
    pub current_app: Option<String>,
    pub history: Vec<String>,
    pub hour_bucket: Option<u8>,
    pub weekday: Option<u8>,
    pub event_type: Option<String>,
}

pub struct NextAppPredictor {
    artifact: NextAppModelArtifact,
    app_index: BTreeMap<String, usize>,
}

impl NextAppPredictor {
    pub fn new(artifact: NextAppModelArtifact) -> Result<Self, String> {
        validate_artifact(&artifact)?;
        let app_index = artifact
            .app_vocab
            .iter()
            .enumerate()
            .map(|(i, app)| (app.clone(), i))
            .collect();
        Ok(Self {
            artifact,
            app_index,
        })
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, String> {
        let file = File::open(path.as_ref())
            .map_err(|err| format!("opening model artifact {}: {err}", path.as_ref().display()))?;
        let artifact: NextAppModelArtifact = serde_json::from_reader(BufReader::new(file))
            .map_err(|err| format!("parsing model artifact {}: {err}", path.as_ref().display()))?;
        Self::new(artifact)
    }

    pub fn artifact(&self) -> &NextAppModelArtifact {
        &self.artifact
    }

    pub fn rank(
        &self,
        features: &PredictionFeatures,
        algorithm: NextAppAlgorithm,
        k: usize,
    ) -> Vec<AppScore> {
        let mut scores = match algorithm {
            NextAppAlgorithm::NaiveBayes => self.rank_naive_bayes(features),
            NextAppAlgorithm::Markov => self.rank_markov(features),
            NextAppAlgorithm::Xgboost => self.rank_xgboost(features),
            NextAppAlgorithm::Ensemble => self.rank_ensemble(features),
        };
        if let Some(current) = features.current_app.as_deref() {
            scores.retain(|score| score.app != current);
        }
        if scores.is_empty() {
            scores = self.artifact.global_popularity.clone();
        }
        scores.truncate(k);
        scores
    }

    fn rank_naive_bayes(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        let mut scores = self.artifact.naive_bayes.class_log_priors.clone();
        for feature_key in feature_keys(features) {
            if let Some(log_probs) = self
                .artifact
                .naive_bayes
                .feature_log_probs
                .get(&feature_key)
            {
                add_vec(&mut scores, log_probs);
            } else {
                add_vec(
                    &mut scores,
                    &self.artifact.naive_bayes.unknown_feature_log_probs,
                );
            }
        }
        rank_from_logits(&self.artifact.app_vocab, &scores)
    }

    fn rank_markov(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        if let (Some(user), Some(current)) = (&features.user_id, &features.current_app) {
            let key = user_transition_key(user, current);
            if let Some(scores) = self.artifact.markov.user_transitions.get(&key) {
                return scores.clone();
            }
        }
        if let Some(current) = &features.current_app {
            if let Some(scores) = self.artifact.markov.global_transitions.get(current) {
                return scores.clone();
            }
        }
        if let Some(prev) = features.history.last() {
            if let Some(scores) = self.artifact.markov.global_transitions.get(prev) {
                return scores.clone();
            }
        }
        self.artifact.global_popularity.clone()
    }

    fn rank_xgboost(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        let active: BTreeSet<String> = feature_keys(features).into_iter().collect();
        let mut scores = self.artifact.xgboost.base_scores.clone();
        for tree in &self.artifact.xgboost.trees {
            if active.contains(&tree.feature_key) {
                for app_score in &tree.yes_scores {
                    if let Some(index) = self.app_index.get(&app_score.app) {
                        scores[*index] += app_score.score;
                    }
                }
            }
        }
        rank_from_logits(&self.artifact.app_vocab, &scores)
    }

    fn rank_ensemble(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        let mut combined: BTreeMap<String, f32> = BTreeMap::new();
        for (weight, scores) in [
            (0.30, self.rank_naive_bayes(features)),
            (0.40, self.rank_markov(features)),
            (0.30, self.rank_xgboost(features)),
        ] {
            for score in scores.into_iter().take(10) {
                *combined.entry(score.app).or_default() += weight * score.score;
            }
        }
        let mut ranked: Vec<AppScore> = combined
            .into_iter()
            .map(|(app, score)| AppScore { app, score })
            .collect();
        ranked.sort_by(|a, b| score_order(a.score, b.score).then_with(|| a.app.cmp(&b.app)));
        ranked
    }
}

pub struct PredictiveLocalBackend {
    predictor: NextAppPredictor,
}

impl PredictiveLocalBackend {
    pub fn new(artifact: NextAppModelArtifact) -> Result<Self, String> {
        Ok(Self {
            predictor: NextAppPredictor::new(artifact)?,
        })
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, String> {
        Ok(Self {
            predictor: NextAppPredictor::from_path(path)?,
        })
    }
}

impl DecisionBackend for PredictiveLocalBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        let input = ModelInput::current_only(context.clone());
        self.evaluate_model_input(&input)
    }

    fn evaluate_model_input(&self, input: &ModelInput) -> DecisionBackendResult {
        let start = Instant::now();
        let features = features_from_model_input(input);
        let known = known_packages(&input.current_context);
        let predictions =
            self.predictor
                .rank(&features, NextAppAlgorithm::Ensemble, MAX_BACKEND_INTENTS);

        let mut intents: Vec<Intent> = predictions
            .into_iter()
            .map(|prediction| {
                let in_context = known.contains(&prediction.app);
                Intent {
                    intent_id: new_id(),
                    intent_type: if features.current_app.is_some() {
                        IntentType::SwitchToApp(prediction.app.clone())
                    } else {
                        IntentType::OpenApp(prediction.app.clone())
                    },
                    confidence: prediction.score.clamp(0.05, 0.99),
                    risk_level: RiskLevel::Low,
                    suggested_actions: vec![SuggestedAction {
                        action_type: if in_context {
                            ActionType::PreWarmProcess
                        } else {
                            ActionType::KeepAlive
                        },
                        target: Some(if in_context {
                            format!("pkg:{}", prediction.app)
                        } else {
                            "work:collector_heartbeat".to_string()
                        }),
                        urgency: ActionUrgency::Immediate,
                    }],
                    rationale_tags: vec![
                        "predictive:next_app".into(),
                        if in_context {
                            "predictive:target_in_context".into()
                        } else {
                            "predictive:target_not_in_context_safe_keepalive".into()
                        },
                    ],
                }
            })
            .collect();

        if intents.is_empty() {
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::Idle,
                confidence: 0.50,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::NoOp,
                    target: None,
                    urgency: ActionUrgency::IdleTime,
                }],
                rationale_tags: vec!["predictive:idle_no_prediction".into()],
            });
        }

        let intent_batch = IntentBatch {
            window_id: input.current_context.window_id.clone(),
            intents,
            generated_at_ms: input.current_context.window_end_ms,
            model: MODEL_NAME.into(),
        };
        let rationale_tags = intent_batch
            .intents
            .iter()
            .flat_map(|intent| intent.rationale_tags.iter().cloned())
            .collect();

        DecisionBackendResult {
            route: DecisionRoute::LocalEvaluator,
            intent_batch,
            rationale_tags,
            latency_us: start.elapsed().as_micros() as u64,
            error: None,
        }
    }
}

pub fn train_next_app_artifact(
    dataset_id: impl Into<String>,
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

    for example in examples {
        let label_idx = *app_index
            .get(&example.label_app)
            .ok_or_else(|| format!("label not in vocab: {}", example.label_app))?;
        class_counts[label_idx] += 1;

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
            feature_counts.get(&feature).map(|counts| XgboostTree {
                feature_key: feature,
                yes_scores: counts_to_log_lift_scores(counts, &class_counts, &app_vocab),
            })
        })
        .collect();

    Ok(NextAppModelArtifact {
        schema_version: SCHEMA_VERSION.into(),
        model_id: MODEL_NAME.into(),
        dataset_id: dataset_id.into(),
        trained_at_ms: now_ms(),
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
        },
        xgboost: XgboostModel {
            base_scores: class_log_priors,
            trees,
        },
        training_summary: TrainingSummary {
            examples: examples.len(),
            users: user_set.len(),
            apps: classes,
        },
    })
}

pub fn prediction_features_for_example(example: &NextAppTrainingExample) -> PredictionFeatures {
    PredictionFeatures {
        user_id: Some(example.user_id.clone()),
        current_app: Some(example.current_app.clone()),
        history: example.history.clone(),
        hour_bucket: Some(example.hour_bucket),
        weekday: Some(example.weekday),
        event_type: Some(example.event_type.clone()),
    }
}

fn validate_artifact(artifact: &NextAppModelArtifact) -> Result<(), String> {
    if artifact.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported next-app artifact schema {}; expected {SCHEMA_VERSION}",
            artifact.schema_version
        ));
    }
    let classes = artifact.app_vocab.len();
    if classes == 0 {
        return Err("artifact app_vocab is empty".into());
    }
    if artifact.naive_bayes.class_log_priors.len() != classes
        || artifact.naive_bayes.unknown_feature_log_probs.len() != classes
        || artifact.xgboost.base_scores.len() != classes
    {
        return Err("artifact vector sizes do not match app_vocab".into());
    }
    Ok(())
}

fn features_from_model_input(input: &ModelInput) -> PredictionFeatures {
    let context = &input.current_context;
    let current_app =
        latest_foreground_app(context).or_else(|| context.summary.foreground_apps.last().cloned());
    let mut history: Vec<String> = input
        .recent_feedback
        .iter()
        .rev()
        .flat_map(|record| record.foreground_apps.iter().rev().cloned())
        .take(5)
        .collect();
    history.reverse();
    let event_type = context.events.last().map(|event| {
        match &event.event_type {
            SanitizedEventType::AppTransition { .. } => "app_transition",
            SanitizedEventType::Notification { .. } => "notification",
            SanitizedEventType::FileActivity { .. } => "file_activity",
            SanitizedEventType::Screen { .. } => "screen",
            SanitizedEventType::SystemStatus { .. } => "system_status",
            SanitizedEventType::ProcessResource { .. } => "process_resource",
            SanitizedEventType::InterAppInteraction { .. } => "inter_app",
        }
        .to_string()
    });
    PredictionFeatures {
        user_id: None,
        current_app,
        history,
        hour_bucket: Some(hour_bucket(context.window_end_ms)),
        weekday: Some(weekday(context.window_end_ms)),
        event_type,
    }
}

fn latest_foreground_app(context: &StructuredContext) -> Option<String> {
    context
        .events
        .iter()
        .rev()
        .find_map(|event| match &event.event_type {
            SanitizedEventType::AppTransition {
                package_name,
                transition: AppTransition::Foreground,
                ..
            } => Some(package_name.clone()),
            _ => None,
        })
}

fn known_packages(context: &StructuredContext) -> BTreeSet<String> {
    let mut packages = BTreeSet::new();
    packages.extend(context.summary.foreground_apps.iter().cloned());
    packages.extend(context.summary.notified_apps.iter().cloned());
    for event in &context.events {
        if let Some(pkg) = &event.app_package {
            packages.insert(pkg.clone());
        }
        match &event.event_type {
            SanitizedEventType::AppTransition { package_name, .. } => {
                packages.insert(package_name.clone());
            },
            SanitizedEventType::Notification { source_package, .. } => {
                packages.insert(source_package.clone());
            },
            SanitizedEventType::ProcessResource {
                package_name: Some(package),
                ..
            } => {
                packages.insert(package.clone());
            },
            _ => {},
        }
    }
    packages
}

fn training_features(example: &NextAppTrainingExample) -> Vec<String> {
    feature_keys(&prediction_features_for_example(example))
}

fn feature_keys(features: &PredictionFeatures) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(user) = &features.user_id {
        keys.push(format!("user={user}"));
    }
    if let Some(current) = &features.current_app {
        keys.push(format!("current={current}"));
    }
    if let Some(prev) = features.history.last() {
        keys.push(format!("prev={prev}"));
    }
    for (idx, app) in features.history.iter().rev().take(3).enumerate() {
        keys.push(format!("hist{idx}={app}"));
    }
    if let Some(hour) = features.hour_bucket {
        keys.push(format!("hour={hour}"));
    }
    if let Some(weekday) = features.weekday {
        keys.push(format!("weekday={weekday}"));
    }
    if let Some(event_type) = &features.event_type {
        keys.push(format!("event={event_type}"));
    }
    keys
}

fn add_vec(target: &mut [f32], values: &[f32]) {
    for (target, value) in target.iter_mut().zip(values.iter()) {
        *target += *value;
    }
}

fn rank_from_logits(app_vocab: &[String], logits: &[f32]) -> Vec<AppScore> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exp: Vec<f32> = logits.iter().map(|score| (*score - max).exp()).collect();
    let sum: f32 = exp.iter().sum();
    let mut ranked: Vec<AppScore> = app_vocab
        .iter()
        .cloned()
        .zip(exp)
        .map(|(app, value)| AppScore {
            app,
            score: if sum > 0.0 { value / sum } else { 0.0 },
        })
        .collect();
    ranked.sort_by(|a, b| score_order(a.score, b.score).then_with(|| a.app.cmp(&b.app)));
    ranked
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

fn user_transition_key(user_id: &str, current_app: &str) -> String {
    format!("{user_id}\t{current_app}")
}

fn score_order(a: f32, b: f32) -> std::cmp::Ordering {
    b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

fn hour_bucket(timestamp_ms: i64) -> u8 {
    let seconds = timestamp_ms.div_euclid(1000);
    ((seconds.div_euclid(3600)).rem_euclid(24)) as u8
}

fn weekday(timestamp_ms: i64) -> u8 {
    let days = timestamp_ms.div_euclid(86_400_000);
    ((days + 4).rem_euclid(7)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use aios_spec::{
        ContextSummary, SanitizedEvent, SourceTier, StructuredContext, SystemStatusSnapshot,
    };

    fn examples() -> Vec<NextAppTrainingExample> {
        vec![
            example("u1", "com.chat", &[], "com.mail"),
            example("u1", "com.chat", &["com.home"], "com.mail"),
            example("u2", "com.chat", &[], "com.mail"),
            example("u2", "com.mail", &["com.chat"], "com.browser"),
            example("u3", "com.chat", &[], "com.browser"),
        ]
    }

    fn example(
        user_id: &str,
        current_app: &str,
        history: &[&str],
        label_app: &str,
    ) -> NextAppTrainingExample {
        NextAppTrainingExample {
            user_id: user_id.into(),
            current_app: current_app.into(),
            history: history.iter().map(|app| (*app).into()).collect(),
            hour_bucket: 9,
            weekday: 1,
            event_type: "app_usage".into(),
            label_app: label_app.into(),
        }
    }

    #[test]
    fn markov_ranks_observed_transition_first() {
        let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
            .expect("training should succeed");
        let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");
        let features = PredictionFeatures {
            current_app: Some("com.chat".into()),
            ..PredictionFeatures::default()
        };

        let ranked = predictor.rank(&features, NextAppAlgorithm::Markov, 3);

        assert_eq!(ranked[0].app, "com.mail");
        assert!(ranked[0].score > ranked[1].score);
    }

    #[test]
    fn malformed_artifact_is_rejected() {
        let mut artifact =
            train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
                .expect("training should succeed");
        artifact.naive_bayes.class_log_priors.pop();

        assert!(NextAppPredictor::new(artifact).is_err());
    }

    #[test]
    fn backend_emits_policy_safe_action_for_unobserved_prediction() {
        let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
            .expect("training should succeed");
        let backend = PredictiveLocalBackend::new(artifact).expect("backend should construct");
        let ctx = context_with_foreground("com.chat");

        let result = backend.evaluate(&ctx);
        let first = &result.intent_batch.intents[0];

        assert_eq!(result.route, DecisionRoute::LocalEvaluator);
        assert!(matches!(first.intent_type, IntentType::SwitchToApp(_)));
        assert_eq!(first.risk_level, RiskLevel::Low);
        assert_eq!(
            first.suggested_actions[0].action_type,
            ActionType::KeepAlive
        );
        assert_eq!(
            first.suggested_actions[0].target.as_deref(),
            Some("work:collector_heartbeat")
        );
    }

    fn context_with_foreground(package: &str) -> StructuredContext {
        StructuredContext {
            window_id: "w1".into(),
            window_start_ms: 0,
            window_end_ms: 1_000,
            duration_secs: 1,
            events: vec![SanitizedEvent {
                event_id: "e1".into(),
                timestamp_ms: 1_000,
                event_type: SanitizedEventType::AppTransition {
                    package_name: package.into(),
                    activity_class: None,
                    transition: AppTransition::Foreground,
                },
                source_tier: SourceTier::PublicApi,
                app_package: Some(package.into()),
                uid: None,
            }],
            summary: ContextSummary {
                foreground_apps: vec![package.into()],
                notified_apps: vec![],
                all_semantic_hints: vec![],
                file_activity: vec![],
                latest_system_status: Option::<SystemStatusSnapshot>::None,
                source_tier: SourceTier::PublicApi,
            },
        }
    }
}
