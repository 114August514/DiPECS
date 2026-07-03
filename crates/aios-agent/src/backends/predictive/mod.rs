//! PredictiveLocalBackend - deterministic next-app prediction from exported artifacts.
//!
//! Training can happen offline, but runtime inference stays local and pure: the
//! backend reads a JSON artifact containing Naive Bayes, Markov, and a log-lift
//! feature ensemble, then emits low-risk app intent candidates.

mod backend;
mod ensemble;
mod predictor;
mod train;
mod types;

#[cfg(test)]
mod tests;

pub use backend::PredictiveLocalBackend;
pub use train::train_next_app_artifact;
pub use types::{
    AppScore, EnsembleCombiner, FeatureLiftModel, FeatureLiftTree, LogisticRerankerModel,
    MarkovModel, NaiveBayesModel, NextAppAlgorithm, NextAppModelArtifact, NextAppModelConfig,
    NextAppTrainingExample, PredictionFeatures, TrainingSummary,
};

pub struct NextAppPredictor {
    artifact: NextAppModelArtifact,
    app_index: std::collections::BTreeMap<String, usize>,
}

const SCHEMA_VERSION: &str = "dipecs.next_app_model.v1";
const MODEL_NAME: &str = "predictive-local-v0.1";
const MAX_BACKEND_INTENTS: usize = 5;
const MAX_TRAINED_FEATURES: usize = 128;

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

fn score_order(a: f32, b: f32) -> std::cmp::Ordering {
    b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
}

fn hour_bucket(timestamp_ms: i64) -> u8 {
    let seconds = timestamp_ms.div_euclid(1000);
    ((seconds.div_euclid(3600)).rem_euclid(24)) as u8
}

fn weekday(timestamp_ms: i64) -> u8 {
    let days = timestamp_ms.div_euclid(86_400_000);
    ((days + 4).rem_euclid(7)) as u8
}
