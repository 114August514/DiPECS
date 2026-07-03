//! Data types for the next-app predictive model.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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
    /// Log-lift feature ensemble. The JSON field name remains `xgboost` for
    /// backward compatibility with existing artifacts, but the implementation
    /// is a lightweight deterministic feature-lift model, not an XGBoost
    /// gradient-boosted tree ensemble.
    #[serde(rename = "xgboost")]
    pub feature_lift: FeatureLiftModel,
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

/// Lightweight deterministic feature-lift ensemble.
///
/// This is **not** an XGBoost gradient-boosted tree. It selects the top-k most
/// frequent categorical features from the training data and stores per-feature
/// log-lift scores for each app. At inference time the active features' lifts
/// are added to the base (log-prior) scores. The artifact JSON field is still
/// labeled `xgboost` for backward compatibility with existing artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureLiftModel {
    pub base_scores: Vec<f32>,
    pub trees: Vec<FeatureLiftTree>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureLiftTree {
    pub feature_key: String,
    pub yes_scores: Vec<AppScore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextAppAlgorithm {
    NaiveBayes,
    Markov,
    /// Log-lift feature ensemble (the artifact field is still serialized as
    /// `xgboost` for compatibility, but the model is not XGBoost).
    FeatureLift,
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
