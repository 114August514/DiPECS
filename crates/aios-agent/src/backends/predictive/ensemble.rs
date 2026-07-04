//! Ensemble fusion and candidate reranking for next-app prediction.

use std::collections::{BTreeMap, BTreeSet};

use super::train::train_base_artifact;
use super::{
    prediction_features_for_example, score_order, AppScore, EnsembleCombiner,
    LogisticRerankerModel, NextAppModelConfig, NextAppPredictor, NextAppTrainingExample,
    PredictionFeatures,
};

/// Rank-discount smoothing constant for the fusion score `weight / (RRF_K +
/// rank)`. We use k=1.0 (rank 0 -> full weight, rank 1 -> half, ...) rather
/// than the classic search-fusion default of 60 because next-app hit@1 rewards
/// a sharp top-rank preference.
pub(crate) const RRF_K: f32 = 1.0;

const MAGNITUDE_BLEND: f32 = 0.5;

const LOGISTIC_FEATURES_PER_COMPONENT: usize = 3;
const LOGISTIC_NEGATIVES_PER_COMPONENT: usize = 4;
const LOGISTIC_EPOCHS: usize = 5;
const LOGISTIC_LR: f32 = 0.08;
const LOGISTIC_L2: f32 = 0.0001;
const MIN_LOGISTIC_CACHED_EXAMPLES: usize = 20;

/// Grid of candidate weights swept per component during coordinate ascent.
const WEIGHT_GRID: &[f32] = &[
    0.0, 0.05, 0.1, 0.2, 0.4, 0.8, 1.2, 1.6, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0,
];
const MAX_ASCENT_SWEEPS: usize = 6;
const VAL_MODULUS: usize = 20;
const VAL_CUTOFF: usize = 3;

pub(super) struct EnsembleModels {
    pub rrf: EnsembleCombiner,
    pub logistic: LogisticRerankerModel,
}

struct EnsembleComponent {
    name: &'static str,
    rank: fn(&NextAppPredictor, &PredictionFeatures) -> Vec<AppScore>,
}

#[derive(Clone)]
struct CachedExample {
    label: String,
    rankings: Vec<Vec<AppScore>>,
    rows: BTreeMap<String, Vec<f32>>,
}

impl NextAppPredictor {
    pub(super) fn rank_ensemble(&self, features: &PredictionFeatures) -> Vec<AppScore> {
        if !self.artifact.ensemble_logistic.is_empty() {
            return self.rank_ensemble_logistic(features, &self.artifact.ensemble_logistic);
        }
        if self.artifact.ensemble_combiner.is_empty() {
            return self.rank_ensemble_legacy(features);
        }
        self.rank_ensemble_rrf(features, &self.artifact.ensemble_combiner)
    }

    fn rank_ensemble_logistic(
        &self,
        features: &PredictionFeatures,
        model: &LogisticRerankerModel,
    ) -> Vec<AppScore> {
        let rankings = self.component_rankings(features);
        let rows = candidate_feature_rows(&rankings, features.current_app.as_deref());
        let mut ranked: Vec<AppScore> = rows
            .into_iter()
            .map(|(app, row)| AppScore {
                app,
                score: sigmoid(dot(&model.weights, &row)),
            })
            .collect();
        ranked.sort_by(|a, b| score_order(a.score, b.score).then_with(|| a.app.cmp(&b.app)));
        ranked
    }

    fn rank_ensemble_legacy(&self, features: &PredictionFeatures) -> Vec<AppScore> {
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

    fn rank_ensemble_rrf(
        &self,
        features: &PredictionFeatures,
        combiner: &EnsembleCombiner,
    ) -> Vec<AppScore> {
        let mut combined: BTreeMap<String, f32> = BTreeMap::new();
        for component in ensemble_components() {
            let Some(weight) = combiner.weight_of(component.name) else {
                continue;
            };
            if weight == 0.0 {
                continue;
            }
            let ranked = (component.rank)(self, features);
            for (rank, score) in ranked.iter().enumerate() {
                let rr = 1.0 / (RRF_K + rank as f32);
                let contribution = weight * (rr + MAGNITUDE_BLEND * score.score);
                *combined.entry(score.app.clone()).or_default() += contribution;
            }
        }
        let mut ranked: Vec<AppScore> = combined
            .into_iter()
            .map(|(app, score)| AppScore { app, score })
            .collect();
        ranked.sort_by(|a, b| score_order(a.score, b.score).then_with(|| a.app.cmp(&b.app)));
        ranked
    }

    pub(crate) fn component_rankings(
        &self,
        features: &PredictionFeatures,
    ) -> Vec<(&'static str, Vec<AppScore>)> {
        ensemble_components()
            .iter()
            .map(|component| (component.name, (component.rank)(self, features)))
            .collect()
    }
}

pub(crate) fn ensemble_component_names() -> Vec<&'static str> {
    ensemble_components().iter().map(|c| c.name).collect()
}

pub(crate) fn logistic_feature_names() -> Vec<String> {
    ensemble_component_names()
        .into_iter()
        .flat_map(|component| {
            [
                format!("{component}:rank_rr"),
                format!("{component}:score"),
                format!("{component}:top1"),
            ]
        })
        .collect()
}

pub(super) fn fit_ensemble_models(
    dataset_id: &str,
    config: &NextAppModelConfig,
    examples: &[NextAppTrainingExample],
) -> EnsembleModels {
    let mut fit_examples = Vec::new();
    let mut val_examples = Vec::new();
    for (idx, example) in examples.iter().enumerate() {
        if idx % VAL_MODULUS < VAL_CUTOFF {
            val_examples.push(example.clone());
        } else {
            fit_examples.push(example.clone());
        }
    }
    if fit_examples.is_empty() || val_examples.is_empty() {
        return EnsembleModels {
            rrf: EnsembleCombiner::default(),
            logistic: LogisticRerankerModel::default(),
        };
    }

    let Ok(fit_artifact) = train_base_artifact(dataset_id, config.clone(), &fit_examples) else {
        return EnsembleModels {
            rrf: EnsembleCombiner::default(),
            logistic: LogisticRerankerModel::default(),
        };
    };
    let Ok(fit_predictor) = NextAppPredictor::new(fit_artifact) else {
        return EnsembleModels {
            rrf: EnsembleCombiner::default(),
            logistic: LogisticRerankerModel::default(),
        };
    };

    let cached = cache_validation_examples(&fit_predictor, &val_examples);
    let rrf = fit_rrf_combiner(&cached);
    let logistic_fit: Vec<CachedExample> = cached
        .iter()
        .enumerate()
        .filter(|(idx, _)| idx % 2 == 0)
        .map(|(_, example)| example.clone())
        .collect();
    let logistic_gate: Vec<CachedExample> = cached
        .iter()
        .enumerate()
        .filter(|(idx, _)| idx % 2 == 1)
        .map(|(_, example)| example.clone())
        .collect();
    let mut logistic = fit_logistic_reranker(&logistic_fit);
    if !logistic.is_empty()
        && (logistic_gate.is_empty()
            || logistic_hit_at_1(&logistic_gate, &logistic)
                < weighted_hit_at_1(&logistic_gate, &rrf.weights))
    {
        logistic = LogisticRerankerModel::default();
    }
    EnsembleModels { rrf, logistic }
}

fn cache_validation_examples(
    predictor: &NextAppPredictor,
    examples: &[NextAppTrainingExample],
) -> Vec<CachedExample> {
    examples
        .iter()
        .filter(|example| example.label_app != example.current_app)
        .filter_map(|example| {
            let features = prediction_features_for_example(example);
            let rankings = predictor.component_rankings(&features);
            let rows = candidate_feature_rows(&rankings, Some(&example.current_app));
            if !rows.contains_key(&example.label_app) {
                return None;
            }
            let rankings = rankings
                .into_iter()
                .map(|(_, scores)| {
                    scores
                        .into_iter()
                        .filter(|s| s.app != example.current_app)
                        .collect::<Vec<_>>()
                })
                .collect();
            Some(CachedExample {
                label: example.label_app.clone(),
                rankings,
                rows,
            })
        })
        .collect()
}

fn fit_rrf_combiner(cached: &[CachedExample]) -> EnsembleCombiner {
    if cached.is_empty() {
        return EnsembleCombiner::default();
    }
    let components = ensemble_component_names();
    let mut weights: Vec<f32> = components
        .iter()
        .map(|name| match *name {
            "markov_order2" => 1.2,
            "markov" => 0.8,
            "markov_context" => 0.5,
            "recency" => 0.45,
            "naive_bayes" => 0.6,
            "feature_lift" => 0.3,
            "popularity" => 0.05,
            _ => 0.2,
        })
        .collect();

    let mut best_hit = weighted_hit_at_1(cached, &weights);
    for _ in 0..MAX_ASCENT_SWEEPS {
        let mut improved = false;
        for c in 0..weights.len() {
            let original = weights[c];
            let mut best_w = original;
            for &candidate in WEIGHT_GRID {
                if candidate == original {
                    continue;
                }
                weights[c] = candidate;
                let hit = weighted_hit_at_1(cached, &weights);
                if hit > best_hit {
                    best_hit = hit;
                    best_w = candidate;
                    improved = true;
                }
            }
            weights[c] = best_w;
        }
        if !improved {
            break;
        }
    }

    EnsembleCombiner {
        components: components.iter().map(|s| s.to_string()).collect(),
        weights,
    }
}

fn fit_logistic_reranker(cached: &[CachedExample]) -> LogisticRerankerModel {
    if cached.len() < MIN_LOGISTIC_CACHED_EXAMPLES {
        return LogisticRerankerModel::default();
    }

    let feature_names = logistic_feature_names();
    let mut weights = vec![0.0; feature_names.len()];
    let mut updates = 0usize;

    for epoch in 0..LOGISTIC_EPOCHS {
        let lr = LOGISTIC_LR / (1.0 + epoch as f32 * 0.5);
        for example in cached {
            let Some(pos) = example.rows.get(&example.label) else {
                continue;
            };
            for neg_app in hard_negatives(example) {
                let Some(neg) = example.rows.get(&neg_app) else {
                    continue;
                };
                let logit = dot_diff(&weights, pos, neg);
                let grad = 1.0 - sigmoid(logit);
                for idx in 0..weights.len() {
                    let diff = pos[idx] - neg[idx];
                    if diff != 0.0 {
                        weights[idx] += lr * (grad * diff - LOGISTIC_L2 * weights[idx]);
                    }
                }
                updates += 1;
            }
        }
    }

    if updates == 0 {
        LogisticRerankerModel::default()
    } else {
        LogisticRerankerModel {
            feature_names,
            weights,
        }
    }
}

fn hard_negatives(example: &CachedExample) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut negatives = Vec::new();
    for list in &example.rankings {
        for app in list.iter().take(LOGISTIC_NEGATIVES_PER_COMPONENT) {
            if app.app == example.label || !seen.insert(app.app.as_str()) {
                continue;
            }
            negatives.push(app.app.clone());
        }
    }
    if negatives.is_empty() {
        for app in example.rows.keys() {
            if app != &example.label {
                negatives.push(app.clone());
                break;
            }
        }
    }
    negatives
}

fn weighted_hit_at_1(cached: &[CachedExample], weights: &[f32]) -> f64 {
    let mut hits = 0usize;
    for example in cached {
        let mut combined: BTreeMap<&str, f32> = BTreeMap::new();
        for (component_idx, list) in example.rankings.iter().enumerate() {
            let weight = weights[component_idx];
            if weight == 0.0 {
                continue;
            }
            for (rank, score) in list.iter().enumerate() {
                let rr = 1.0 / (RRF_K + rank as f32);
                let contribution = weight * (rr + MAGNITUDE_BLEND * score.score);
                *combined.entry(score.app.as_str()).or_default() += contribution;
            }
        }
        let top = combined.into_iter().max_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.0.cmp(a.0))
        });
        if let Some((app, _)) = top {
            if app == example.label.as_str() {
                hits += 1;
            }
        }
    }
    hits as f64 / cached.len().max(1) as f64
}

fn logistic_hit_at_1(cached: &[CachedExample], model: &LogisticRerankerModel) -> f64 {
    if cached.is_empty() || model.is_empty() {
        return 0.0;
    }
    let mut hits = 0usize;
    for example in cached {
        let top = example
            .rows
            .iter()
            .max_by(|(app_a, row_a), (app_b, row_b)| {
                let score_a = sigmoid(dot(&model.weights, row_a));
                let score_b = sigmoid(dot(&model.weights, row_b));
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| app_b.cmp(app_a))
            });
        if let Some((app, _)) = top {
            if app == &example.label {
                hits += 1;
            }
        }
    }
    hits as f64 / cached.len() as f64
}

fn candidate_feature_rows(
    rankings: &[(&'static str, Vec<AppScore>)],
    current_app: Option<&str>,
) -> BTreeMap<String, Vec<f32>> {
    let feature_len = logistic_feature_names().len();
    let mut rows: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for (component_idx, (_name, scores)) in rankings.iter().enumerate() {
        let offset = component_idx * LOGISTIC_FEATURES_PER_COMPONENT;
        for (rank, score) in scores.iter().enumerate() {
            if current_app == Some(score.app.as_str()) {
                continue;
            }
            let row = rows
                .entry(score.app.clone())
                .or_insert_with(|| vec![0.0; feature_len]);
            row[offset] = 1.0 / (1.0 + rank as f32);
            row[offset + 1] = score.score;
            row[offset + 2] = if rank == 0 { 1.0 } else { 0.0 };
        }
    }
    rows
}

fn ensemble_components() -> &'static [EnsembleComponent] {
    &[
        EnsembleComponent {
            name: "naive_bayes",
            rank: NextAppPredictor::rank_naive_bayes,
        },
        EnsembleComponent {
            name: "markov_order2",
            rank: NextAppPredictor::rank_markov_order2,
        },
        EnsembleComponent {
            name: "markov",
            rank: NextAppPredictor::rank_markov,
        },
        EnsembleComponent {
            name: "markov_context",
            rank: NextAppPredictor::rank_markov_context,
        },
        EnsembleComponent {
            name: "feature_lift",
            rank: NextAppPredictor::rank_feature_lift,
        },
        EnsembleComponent {
            name: "recency",
            rank: NextAppPredictor::rank_recency,
        },
        EnsembleComponent {
            name: "user_frequency",
            rank: NextAppPredictor::rank_user_frequency,
        },
        EnsembleComponent {
            name: "user_recency",
            rank: NextAppPredictor::rank_user_recency,
        },
        EnsembleComponent {
            name: "popularity",
            rank: NextAppPredictor::rank_popularity,
        },
    ]
}

fn dot(weights: &[f32], row: &[f32]) -> f32 {
    weights
        .iter()
        .zip(row.iter())
        .map(|(weight, value)| weight * value)
        .sum()
}

fn dot_diff(weights: &[f32], pos: &[f32], neg: &[f32]) -> f32 {
    weights
        .iter()
        .zip(pos.iter().zip(neg.iter()))
        .map(|(weight, (pos, neg))| weight * (pos - neg))
        .sum()
}

fn sigmoid(logit: f32) -> f32 {
    if logit >= 0.0 {
        1.0 / (1.0 + (-logit).exp())
    } else {
        let exp = logit.exp();
        exp / (1.0 + exp)
    }
}
