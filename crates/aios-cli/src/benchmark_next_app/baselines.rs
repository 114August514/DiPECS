//! Simple baseline predictors for the next-app benchmark.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use rand::seq::SliceRandom;
use rand::{rngs::StdRng, SeedableRng};

use super::types::{NextAppLabel, NextAppPredictor, PredictionResult, ScoredPrediction};

/// Always predict nothing (empty ranked list) — the simplest NoOp baseline.
pub struct AlwaysNoOpBackend;

impl NextAppPredictor for AlwaysNoOpBackend {
    fn name(&self) -> &'static str {
        "always_noop"
    }

    fn predict(
        &self,
        _ctx: &aios_spec::StructuredContext,
        _current_app: &str,
        _candidates: &[String],
    ) -> PredictionResult {
        PredictionResult {
            ranked: Vec::new(),
            latency_us: 0,
        }
    }
}

/// Randomly shuffle the observable candidates with a fixed seed.
pub struct RandomCandidateBackend {
    rng: RefCell<StdRng>,
}

impl RandomCandidateBackend {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: RefCell::new(StdRng::seed_from_u64(seed)),
        }
    }
}

impl NextAppPredictor for RandomCandidateBackend {
    fn name(&self) -> &'static str {
        "random_candidate"
    }

    fn predict(
        &self,
        _ctx: &aios_spec::StructuredContext,
        _current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let start = Instant::now();
        let mut shuffled = candidates.to_vec();
        let mut rng = self.rng.borrow_mut();
        shuffled.shuffle(&mut *rng);
        PredictionResult {
            ranked: shuffled
                .into_iter()
                .map(|package| ScoredPrediction {
                    package,
                    score: 1.0,
                })
                .collect(),
            latency_us: start.elapsed().as_micros() as u64,
        }
    }
}

/// Always pick the first observable candidate.
pub struct FirstCandidateBackend;

impl NextAppPredictor for FirstCandidateBackend {
    fn name(&self) -> &'static str {
        "first_candidate"
    }

    fn predict(
        &self,
        _ctx: &aios_spec::StructuredContext,
        _current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let start = Instant::now();
        PredictionResult {
            ranked: candidates
                .first()
                .cloned()
                .into_iter()
                .map(|package| ScoredPrediction {
                    package,
                    score: 1.0,
                })
                .collect(),
            latency_us: start.elapsed().as_micros() as u64,
        }
    }
}

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
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.package.cmp(&b.package))
        });
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
        }
    }
}

fn rank_by_counts(candidates: &[String], counts: &HashMap<String, u32>) -> Vec<ScoredPrediction> {
    let mut scored: Vec<ScoredPrediction> = candidates
        .iter()
        .map(|package| ScoredPrediction {
            package: package.clone(),
            score: counts.get(package).copied().unwrap_or(0) as f32,
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.package.cmp(&b.package))
    });
    scored
}
