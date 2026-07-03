//! Next-app predictor and ranking algorithms.

use std::collections::{BTreeMap, BTreeSet};

use super::train::user_transition_key;
use super::{
    score_order, AppScore, NextAppAlgorithm, NextAppModelArtifact, NextAppPredictor,
    PredictionFeatures, SCHEMA_VERSION,
};

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

    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let file = std::fs::File::open(path.as_ref())
            .map_err(|err| format!("opening model artifact {}: {err}", path.as_ref().display()))?;
        let artifact: NextAppModelArtifact = serde_json::from_reader(std::io::BufReader::new(file))
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
            NextAppAlgorithm::FeatureLift => self.rank_feature_lift(features),
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

    fn rank_feature_lift(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        let active: BTreeSet<String> = feature_keys(features).into_iter().collect();
        let mut scores = self.artifact.feature_lift.base_scores.clone();
        for tree in &self.artifact.feature_lift.trees {
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
            (0.30, self.rank_feature_lift(features)),
        ] {
            for score in scores {
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
        || artifact.feature_lift.base_scores.len() != classes
        || artifact
            .feature_lift
            .trees
            .iter()
            .any(|tree| tree.yes_scores.len() != classes)
    {
        return Err("artifact vector sizes do not match app_vocab".into());
    }
    if artifact
        .naive_bayes
        .feature_log_probs
        .values()
        .any(|probs| probs.len() != classes)
    {
        return Err(
            "artifact naive_bayes feature_log_probs vector sizes do not match app_vocab".into(),
        );
    }
    Ok(())
}

pub(crate) fn feature_keys(features: &PredictionFeatures) -> Vec<String> {
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
