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
    /// Per-user app-usage frequency ranking (MFU), keyed by user id. Unlike the
    /// per-user Markov table this is unconditional on the current app: it ranks
    /// the apps a user opens most often overall. Defaults to empty for backward
    /// compatibility with older artifacts.
    #[serde(default)]
    pub user_frequency: BTreeMap<String, Vec<AppScore>>,
    /// Hard recency pointer: the single most recent next-app observed per
    /// `"{user}\t{current}"` key in training order (last write wins). Unlike the
    /// per-user Markov distribution this is a one-app "what did this user open
    /// last time from here" signal, a strong top-1 predictor on repetitive
    /// personal usage. Defaults to empty for backward compatibility.
    #[serde(default)]
    pub user_recency: BTreeMap<String, String>,
    /// Context-aware Markov transitions keyed by temporal features. Keys are
    /// `"{current}\t{hour}"` or `"{current}\t{weekday}"`, values are ranked
    /// next-app distributions. This captures time-of-day and day-of-week
    /// patterns that the global Markov table averages over. Defaults to empty
    /// for backward compatibility with older artifacts.
    #[serde(default)]
    pub markov_context: BTreeMap<String, Vec<AppScore>>,
    /// Learned reciprocal-rank-fusion combiner for the ensemble algorithm. The
    /// weights are fit offline on a held-out validation slice (never on the
    /// test split) and locked into the artifact, so runtime inference is pure
    /// and deterministic with no extra dependencies. Defaults to empty, in
    /// which case the ensemble falls back to the legacy fixed-weight fusion so
    /// older committed artifacts keep working.
    #[serde(default)]
    pub ensemble_combiner: EnsembleCombiner,
    /// Learned logistic candidate reranker over the same component features as
    /// the ensemble. Empty means "use `ensemble_combiner` / legacy fusion".
    #[serde(default)]
    pub ensemble_logistic: LogisticRerankerModel,
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
    /// Global order-2 transitions keyed `"{prev}\t{current}" -> next`. This is
    /// the strongest single next-app signal on LSApp and is global, so it
    /// survives cold-start (unseen users) where per-user tables are empty.
    /// Defaults to empty for backward compatibility with older artifacts.
    #[serde(default)]
    pub global_transitions_order2: BTreeMap<String, Vec<AppScore>>,
}

/// Learned reciprocal-rank-fusion combiner over the ensemble's component
/// rankers. `components` and `weights` are parallel vectors; unknown component
/// names are ignored at inference so the schema can evolve. An empty combiner
/// means "use the legacy fixed-weight fusion".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnsembleCombiner {
    pub components: Vec<String>,
    pub weights: Vec<f32>,
}

impl EnsembleCombiner {
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() || self.components.len() != self.weights.len()
    }

    pub fn weight_of(&self, component: &str) -> Option<f32> {
        self.components
            .iter()
            .position(|c| c == component)
            .map(|idx| self.weights[idx])
    }
}

/// Pairwise-trained logistic reranker over candidate-level ensemble features.
/// `feature_names` and `weights` are parallel vectors; an empty model means the
/// artifact should use the RRF combiner or legacy fixed fusion instead.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogisticRerankerModel {
    pub feature_names: Vec<String>,
    pub weights: Vec<f32>,
}

impl LogisticRerankerModel {
    pub fn is_empty(&self) -> bool {
        self.feature_names.is_empty() || self.feature_names.len() != self.weights.len()
    }
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
