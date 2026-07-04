//! Next-app predictor and ranking algorithms.

use std::collections::BTreeSet;

use super::ensemble::{ensemble_component_names, logistic_feature_names};
use super::train::{order2_key, user_transition_key};
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
            filter_current_app(&mut scores, current);
        }
        if scores.is_empty() {
            scores = self.artifact.global_popularity.clone();
            if let Some(current) = features.current_app.as_deref() {
                filter_current_app(&mut scores, current);
            }
        }
        scores.truncate(k);
        scores
    }

    pub(super) fn rank_naive_bayes(&self, features: &PredictionFeatures) -> Vec<AppScore> {
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

    pub(super) fn rank_markov(&self, features: &PredictionFeatures) -> Vec<AppScore> {
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

    /// Global order-2 Markov ranking keyed on `(prev, current)`. Returns an
    /// empty list when the order-2 pair was never observed, so the combiner can
    /// simply contribute nothing rather than falling back to a weaker signal.
    pub(super) fn rank_markov_order2(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        if let Some(current) = &features.current_app {
            let Some(prev) = previous_feature_app(&features.history, current) else {
                return Vec::new();
            };
            let key = order2_key(prev, current);
            if let Some(scores) = self.artifact.markov.global_transitions_order2.get(&key) {
                return scores.clone();
            }
        }
        Vec::new()
    }

    /// Per-user order-1 Markov ranking (recency proxy). Uses the most recent
    /// `(user, current)` transition distribution; empty when unseen so the
    /// combiner contributes nothing rather than a weaker global signal.
    pub(super) fn rank_recency(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        if let (Some(user), Some(current)) = (&features.user_id, &features.current_app) {
            let key = user_transition_key(user, current);
            if let Some(scores) = self.artifact.markov.user_transitions.get(&key) {
                return scores.clone();
            }
        }
        Vec::new()
    }

    /// Per-user app-usage frequency (MFU) ranking, unconditional on the current
    /// app. Empty when the user was unseen in training, so the combiner
    /// contributes nothing rather than a weaker global signal.
    pub(super) fn rank_user_frequency(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        if let Some(user) = &features.user_id {
            if let Some(scores) = self.artifact.user_frequency.get(user) {
                return scores.clone();
            }
        }
        Vec::new()
    }

    /// Hard recency pointer: the single most recent next-app this user opened
    /// from the current app. Returns a one-element list (score 1.0) so the
    /// combiner gives it a peaked top-1 contribution, mirroring the strong
    /// baseline's flat recency boost. Empty when the `(user, current)` pair was
    /// never seen.
    pub(super) fn rank_user_recency(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        if let (Some(user), Some(current)) = (&features.user_id, &features.current_app) {
            let key = user_transition_key(user, current);
            if let Some(app) = self.artifact.user_recency.get(&key) {
                return vec![AppScore {
                    app: app.clone(),
                    score: 1.0,
                }];
            }
        }
        Vec::new()
    }

    /// Global popularity ranking, used as the combiner's fallback component.
    pub(super) fn rank_popularity(&self, _features: &PredictionFeatures) -> Vec<AppScore> {
        self.artifact.global_popularity.clone()
    }

    /// Context-aware Markov ranking using temporal features. Looks up
    /// `"{current}\t{hour}"` and `"{current}\t{weekday}"` transitions and
    /// merges them with equal weight. Returns empty when no temporal key
    /// matched, so the combiner contributes nothing rather than a weaker
    /// global signal.
    pub(super) fn rank_markov_context(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        let Some(current) = &features.current_app else {
            return Vec::new();
        };
        let mut combined: std::collections::BTreeMap<String, f32> =
            std::collections::BTreeMap::new();
        let mut found = false;
        if let Some(hour) = features.hour_bucket {
            let key = format!("{current}\t{hour}");
            if let Some(scores) = self.artifact.markov_context.get(&key) {
                for s in scores {
                    *combined.entry(s.app.clone()).or_default() += s.score;
                }
                found = true;
            }
        }
        if let Some(weekday) = features.weekday {
            let key = format!("{current}\t{weekday}");
            if let Some(scores) = self.artifact.markov_context.get(&key) {
                for s in scores {
                    *combined.entry(s.app.clone()).or_default() += s.score;
                }
                found = true;
            }
        }
        if !found {
            return Vec::new();
        }
        let mut ranked: Vec<AppScore> = combined
            .into_iter()
            .map(|(app, score)| AppScore { app, score })
            .collect();
        ranked.sort_by(|a, b| score_order(a.score, b.score).then_with(|| a.app.cmp(&b.app)));
        ranked
    }

    pub(super) fn rank_feature_lift(&self, features: &PredictionFeatures) -> Vec<AppScore> {
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
    let mut app_vocab = BTreeSet::new();
    for app in &artifact.app_vocab {
        if !app_vocab.insert(app.as_str()) {
            return Err(format!("artifact app_vocab contains duplicate app `{app}`"));
        }
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
    validate_score_list(
        "global_popularity",
        &artifact.global_popularity,
        &app_vocab,
        true,
    )?;
    for (current_app, scores) in &artifact.markov.global_transitions {
        if !app_vocab.contains(current_app.as_str()) {
            return Err(format!(
                "artifact markov global transition key `{current_app}` is not in app_vocab"
            ));
        }
        validate_score_list(
            &format!("markov.global_transitions[{current_app}]"),
            scores,
            &app_vocab,
            false,
        )?;
    }
    for (key, scores) in &artifact.markov.user_transitions {
        let Some((_, current_app)) = key.rsplit_once('\t') else {
            return Err(format!(
                "artifact markov user transition key `{key}` is not user_id<TAB>current_app"
            ));
        };
        if !app_vocab.contains(current_app) {
            return Err(format!(
                "artifact markov user transition current app `{current_app}` is not in app_vocab"
            ));
        }
        validate_score_list(
            &format!("markov.user_transitions[{key}]"),
            scores,
            &app_vocab,
            false,
        )?;
    }
    for (key, scores) in &artifact.markov.global_transitions_order2 {
        let Some((_, current_app)) = key.rsplit_once('\t') else {
            return Err(format!(
                "artifact markov order2 key `{key}` is not prev_app<TAB>current_app"
            ));
        };
        if !app_vocab.contains(current_app) {
            return Err(format!(
                "artifact markov order2 current app `{current_app}` is not in app_vocab"
            ));
        }
        validate_score_list(
            &format!("markov.global_transitions_order2[{key}]"),
            scores,
            &app_vocab,
            false,
        )?;
    }
    for tree in &artifact.feature_lift.trees {
        validate_score_list(
            &format!("feature_lift.tree[{}].yes_scores", tree.feature_key),
            &tree.yes_scores,
            &app_vocab,
            true,
        )?;
    }
    for (key, scores) in &artifact.markov_context {
        validate_score_list(&format!("markov_context[{key}]"), scores, &app_vocab, false)?;
    }
    for (user_id, scores) in &artifact.user_frequency {
        validate_score_list(
            &format!("user_frequency[{user_id}]"),
            scores,
            &app_vocab,
            false,
        )?;
    }
    for (key, app) in &artifact.user_recency {
        let Some((_, current_app)) = key.rsplit_once('\t') else {
            return Err(format!(
                "artifact user_recency key `{key}` is not user_id<TAB>current_app"
            ));
        };
        if !app_vocab.contains(current_app) {
            return Err(format!(
                "artifact user_recency current app `{current_app}` is not in app_vocab"
            ));
        }
        if !app_vocab.contains(app.as_str()) {
            return Err(format!(
                "artifact user_recency target app `{app}` is not in app_vocab"
            ));
        }
    }
    validate_combiner(&artifact.ensemble_combiner)?;
    if !(artifact.ensemble_logistic.feature_names.is_empty()
        && artifact.ensemble_logistic.weights.is_empty())
    {
        let expected = logistic_feature_names();
        if artifact.ensemble_logistic.feature_names.is_empty()
            || artifact.ensemble_logistic.weights.is_empty()
            || artifact.ensemble_logistic.feature_names.len()
                != artifact.ensemble_logistic.weights.len()
        {
            return Err(
                "artifact ensemble_logistic feature_names and weights must be non-empty parallel vectors"
                    .into(),
            );
        }
        if artifact.ensemble_logistic.feature_names != expected {
            return Err(
                "artifact ensemble_logistic feature_names do not match runtime features".into(),
            );
        }
        if artifact.ensemble_logistic.weights.len() != expected.len() {
            return Err(
                "artifact ensemble_logistic weights length does not match feature_names".into(),
            );
        }
    }
    Ok(())
}

fn validate_combiner(combiner: &super::EnsembleCombiner) -> Result<(), String> {
    if combiner.components.is_empty() && combiner.weights.is_empty() {
        return Ok(());
    }
    if combiner.components.is_empty()
        || combiner.weights.is_empty()
        || combiner.components.len() != combiner.weights.len()
    {
        return Err(
            "artifact ensemble_combiner components and weights must be parallel vectors".into(),
        );
    }
    let known: BTreeSet<&str> = ensemble_component_names().into_iter().collect();
    let mut seen = BTreeSet::new();
    for (component, weight) in combiner.components.iter().zip(combiner.weights.iter()) {
        if !known.contains(component.as_str()) {
            return Err(format!(
                "artifact ensemble_combiner component `{component}` is unknown"
            ));
        }
        if !seen.insert(component.as_str()) {
            return Err(format!(
                "artifact ensemble_combiner contains duplicate component `{component}`"
            ));
        }
        if !weight.is_finite() || *weight < 0.0 {
            return Err(format!(
                "artifact ensemble_combiner weight for `{component}` must be finite and non-negative"
            ));
        }
    }
    Ok(())
}

fn validate_score_list(
    label: &str,
    scores: &[AppScore],
    app_vocab: &BTreeSet<&str>,
    require_full_vocab: bool,
) -> Result<(), String> {
    if require_full_vocab && scores.len() != app_vocab.len() {
        return Err(format!(
            "artifact {label} must contain exactly one score per app_vocab entry"
        ));
    }
    let mut seen = BTreeSet::new();
    for score in scores {
        if !app_vocab.contains(score.app.as_str()) {
            return Err(format!(
                "artifact {label} references app `{}` outside app_vocab",
                score.app
            ));
        }
        if !seen.insert(score.app.as_str()) {
            return Err(format!(
                "artifact {label} contains duplicate score for app `{}`",
                score.app
            ));
        }
    }
    Ok(())
}

fn filter_current_app(scores: &mut Vec<AppScore>, current: &str) {
    scores.retain(|score| score.app != current);
}

fn previous_feature_app<'a>(history: &'a [String], current_app: &str) -> Option<&'a str> {
    history
        .iter()
        .rev()
        .find(|app| app.as_str() != current_app)
        .map(String::as_str)
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
