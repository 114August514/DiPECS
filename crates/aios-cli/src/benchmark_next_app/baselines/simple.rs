//! Simple baseline predictors for the next-app benchmark.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::time::Instant;

use rand::seq::SliceRandom;
use rand::{rngs::StdRng, SeedableRng};

use crate::benchmark_next_app::types::{NextAppPredictor, PredictionResult, ScoredPrediction};

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
            rationale_present: false,
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
            rationale_present: false,
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
            rationale_present: false,
        }
    }
}

/// Compare scored predictions by descending score.
/// Callers add their own tie-breakers for determinism.
pub(crate) fn cmp_score_desc(a: &ScoredPrediction, b: &ScoredPrediction) -> Ordering {
    b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal)
}
