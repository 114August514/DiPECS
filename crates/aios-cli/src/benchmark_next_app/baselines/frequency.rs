//! Frequency-based baseline predictors for the next-app benchmark.

use std::collections::HashMap;
use std::time::Instant;

use crate::benchmark_next_app::types::{
    NextAppLabel, NextAppPredictor, PredictionResult, ScoredPrediction,
};

use super::simple::cmp_score_desc;

/// Always predict the globally most frequent next app seen in training.
#[derive(Default)]
pub struct GlobalMajorityBackend {
    counts: HashMap<String, u32>,
}

impl NextAppPredictor for GlobalMajorityBackend {
    fn name(&self) -> &'static str {
        "global_majority"
    }

    fn train(&mut self, train: &[NextAppLabel]) {
        for label in train {
            if let Some(next) = label.actual_next_app.clone() {
                *self.counts.entry(next).or_insert(0) += 1;
            }
        }
    }

    fn predict(
        &self,
        _ctx: &aios_spec::StructuredContext,
        _current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let start = Instant::now();
        PredictionResult {
            ranked: rank_by_counts(candidates, &self.counts),
            latency_us: start.elapsed().as_micros() as u64,
            rationale_present: false,
        }
    }
}

/// Predict the most frequent next app conditioned on the current app.
#[derive(Default)]
pub struct PerCurrentAppMajorityBackend {
    counts: HashMap<String, HashMap<String, u32>>,
}

impl NextAppPredictor for PerCurrentAppMajorityBackend {
    fn name(&self) -> &'static str {
        "per_current_app_majority"
    }

    fn train(&mut self, train: &[NextAppLabel]) {
        for label in train {
            if let Some(next) = label.actual_next_app.clone() {
                *self
                    .counts
                    .entry(label.current_app.clone())
                    .or_default()
                    .entry(next)
                    .or_insert(0) += 1;
            }
        }
    }

    fn predict(
        &self,
        _ctx: &aios_spec::StructuredContext,
        current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let start = Instant::now();
        let ranked = match self.counts.get(current_app) {
            Some(counts) => rank_by_counts(candidates, counts),
            None => candidates
                .iter()
                .map(|package| ScoredPrediction {
                    package: package.clone(),
                    score: 0.0,
                })
                .collect(),
        };
        PredictionResult {
            ranked,
            latency_us: start.elapsed().as_micros() as u64,
            rationale_present: false,
        }
    }
}

/// First-order Markov: rank by P(next_app | current_app).
#[derive(Default)]
pub struct MarkovBackend {
    transitions: HashMap<String, HashMap<String, u32>>,
    totals: HashMap<String, u32>,
}

impl MarkovBackend {
    fn rank_by_probability(
        candidates: &[String],
        counts: &HashMap<String, u32>,
        total: u32,
    ) -> Vec<ScoredPrediction> {
        let total_f = total.max(1) as f32;
        let mut scored: Vec<ScoredPrediction> = candidates
            .iter()
            .map(|package| ScoredPrediction {
                package: package.clone(),
                score: counts.get(package).copied().unwrap_or(0) as f32 / total_f,
            })
            .collect();
        scored.sort_by(|a, b| cmp_score_desc(a, b).then_with(|| a.package.cmp(&b.package)));
        scored
    }
}

impl NextAppPredictor for MarkovBackend {
    fn name(&self) -> &'static str {
        "markov"
    }

    fn train(&mut self, train: &[NextAppLabel]) {
        for label in train {
            if let Some(next) = label.actual_next_app.clone() {
                *self
                    .transitions
                    .entry(label.current_app.clone())
                    .or_default()
                    .entry(next)
                    .or_insert(0) += 1;
                *self.totals.entry(label.current_app.clone()).or_insert(0) += 1;
            }
        }
    }

    fn predict(
        &self,
        _ctx: &aios_spec::StructuredContext,
        current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let start = Instant::now();
        let ranked = match self.transitions.get(current_app) {
            Some(counts) => {
                let total = self.totals.get(current_app).copied().unwrap_or(0);
                Self::rank_by_probability(candidates, counts, total)
            },
            None => candidates
                .iter()
                .map(|package| ScoredPrediction {
                    package: package.clone(),
                    score: 0.0,
                })
                .collect(),
        };
        PredictionResult {
            ranked,
            latency_us: start.elapsed().as_micros() as u64,
            rationale_present: false,
        }
    }
}

pub(crate) fn rank_by_counts(
    candidates: &[String],
    counts: &HashMap<String, u32>,
) -> Vec<ScoredPrediction> {
    let mut scored: Vec<ScoredPrediction> = candidates
        .iter()
        .map(|package| ScoredPrediction {
            package: package.clone(),
            score: counts.get(package).copied().unwrap_or(0) as f32,
        })
        .collect();
    scored.sort_by(|a, b| cmp_score_desc(a, b).then_with(|| a.package.cmp(&b.package)));
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_majority_ranks_trained_app_first() {
        let mut backend = GlobalMajorityBackend::default();
        backend.train(&[
            next_app_label("A", "B"),
            next_app_label("A", "B"),
            next_app_label("A", "C"),
        ]);
        let result = backend.predict(&empty_ctx(), "A", &["B".into(), "C".into()]);
        assert_eq!(result.ranked[0].package, "B");
        assert_eq!(result.ranked[0].score, 2.0);
    }

    #[test]
    fn markov_uses_conditional_probability() {
        let mut backend = MarkovBackend::default();
        backend.train(&[
            next_app_label("A", "B"),
            next_app_label("A", "B"),
            next_app_label("A", "C"),
        ]);
        let result = backend.predict(&empty_ctx(), "A", &["B".into(), "C".into()]);
        assert_eq!(result.ranked[0].score, 2.0 / 3.0);
        assert_eq!(result.ranked[1].score, 1.0 / 3.0);
    }

    fn next_app_label(current: &str, next: &str) -> NextAppLabel {
        NextAppLabel {
            dataset_id: "d1".into(),
            scenario: "s1".into(),
            window_start_ms: 0,
            window_end_ms: 1,
            prediction_horizon_ms: 30_000,
            current_app: current.into(),
            observable_candidates: vec![next.into()],
            actual_next_app: Some(next.into()),
            eligible: true,
            excluded_reason: None,
        }
    }

    fn empty_ctx() -> aios_spec::StructuredContext {
        aios_spec::StructuredContext {
            window_id: "w1".into(),
            window_start_ms: 0,
            window_end_ms: 1,
            duration_secs: 1,
            events: vec![],
            summary: aios_spec::ContextSummary {
                foreground_apps: vec![],
                notified_apps: vec![],
                all_semantic_hints: vec![],
                file_activity: vec![],
                latest_system_status: None,
                source_tier: aios_spec::SourceTier::PublicApi,
            },
        }
    }
}
