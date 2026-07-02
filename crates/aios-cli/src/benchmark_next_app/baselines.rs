//! Simple baseline predictors for the next-app benchmark.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use rand::seq::SliceRandom;
use rand::{rngs::StdRng, SeedableRng};

use super::types::{NextAppLabel, NextAppPredictor, PredictionResult, ScoredPrediction};
use aios_spec::{AppTransition, SanitizedEventType, SemanticHint};

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
            rationale_present: false,
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

/// Find the most recent non-current foreground `AppTransition` target.
fn last_non_current_foreground(
    ctx: &aios_spec::StructuredContext,
    current_app: &str,
) -> Option<(i64, String)> {
    ctx.events
        .iter()
        .filter_map(|e| match &e.event_type {
            SanitizedEventType::AppTransition {
                package_name,
                transition: AppTransition::Foreground,
                ..
            } => Some((e.timestamp_ms, package_name.clone())),
            _ => None,
        })
        .filter(|(_, package)| package != current_app)
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
}

/// Predict the app that most recently posted a notification.
pub struct RecentNotificationBackend;

impl NextAppPredictor for RecentNotificationBackend {
    fn name(&self) -> &'static str {
        "recent_notification"
    }

    fn predict(
        &self,
        ctx: &aios_spec::StructuredContext,
        _current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let start = Instant::now();
        let ranked = ctx
            .events
            .iter()
            .filter_map(|e| match &e.event_type {
                SanitizedEventType::Notification { source_package, .. } => {
                    Some((e.timestamp_ms, source_package.clone()))
                },
                _ => None,
            })
            .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
            .and_then(|(_, package)| {
                if candidates.contains(&package) {
                    Some(vec![ScoredPrediction {
                        package,
                        score: 1.0,
                    }])
                } else {
                    None
                }
            })
            .unwrap_or_default();
        PredictionResult {
            ranked,
            latency_us: start.elapsed().as_micros() as u64,
            rationale_present: false,
        }
    }
}

/// Predict the most recent non-current foreground app (user switching back).
pub struct LastForegroundBackend;

fn last_foreground_ranked(
    ctx: &aios_spec::StructuredContext,
    current_app: &str,
    candidates: &[String],
) -> Vec<ScoredPrediction> {
    last_non_current_foreground(ctx, current_app)
        .and_then(|(_, package)| {
            if candidates.contains(&package) {
                Some(vec![ScoredPrediction {
                    package,
                    score: 1.0,
                }])
            } else {
                None
            }
        })
        .unwrap_or_default()
}

impl NextAppPredictor for LastForegroundBackend {
    fn name(&self) -> &'static str {
        "last_foreground"
    }

    fn predict(
        &self,
        ctx: &aios_spec::StructuredContext,
        current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let start = Instant::now();
        PredictionResult {
            ranked: last_foreground_ranked(ctx, current_app, candidates),
            latency_us: start.elapsed().as_micros() as u64,
            rationale_present: false,
        }
    }
}

/// Rank candidates by notification priority heuristics.
pub struct NotificationPriorityBackend;

impl NextAppPredictor for NotificationPriorityBackend {
    fn name(&self) -> &'static str {
        "notification_priority"
    }

    fn predict(
        &self,
        ctx: &aios_spec::StructuredContext,
        _current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let start = Instant::now();

        let notifications: Vec<&aios_spec::SanitizedEvent> = ctx
            .events
            .iter()
            .filter(|e| matches!(e.event_type, SanitizedEventType::Notification { .. }))
            .collect();

        if notifications.is_empty() {
            return PredictionResult {
                ranked: Vec::new(),
                latency_us: start.elapsed().as_micros() as u64,
                rationale_present: false,
            };
        }

        let candidate_set: HashSet<String> = candidates.iter().cloned().collect();
        let max_ts = notifications
            .iter()
            .map(|e| e.timestamp_ms)
            .max()
            .unwrap_or(i64::MIN);

        let mut scores: HashMap<String, f32> = HashMap::new();
        let mut latest_ts: HashMap<String, i64> = HashMap::new();

        for event in &notifications {
            let (source_package, category, is_ongoing, semantic_hints) = match &event.event_type {
                SanitizedEventType::Notification {
                    source_package,
                    category,
                    is_ongoing,
                    semantic_hints,
                    ..
                } => (source_package, category, is_ongoing, semantic_hints),
                _ => unreachable!(),
            };

            if !candidate_set.contains(source_package) {
                continue;
            }

            latest_ts
                .entry(source_package.clone())
                .and_modify(|v| *v = (*v).max(event.timestamp_ms))
                .or_insert(event.timestamp_ms);

            let score = scores.entry(source_package.clone()).or_insert(0.0);

            if *is_ongoing {
                *score += 3.0;
            }

            for hint in semantic_hints {
                match hint {
                    SemanticHint::FileMention
                    | SemanticHint::ImageMention
                    | SemanticHint::LinkAttachment => *score += 2.0,
                    SemanticHint::UserMentioned | SemanticHint::CalendarInvitation => *score += 1.0,
                    _ => {},
                }
            }

            if let Some(cat) = category {
                let cat_lower = cat.to_lowercase();
                if matches!(cat_lower.as_str(), "alarm" | "call" | "event") {
                    *score += 1.0;
                }
            }

            if event.timestamp_ms == max_ts {
                *score += 1.0;
            }
        }

        if scores.is_empty() {
            return PredictionResult {
                ranked: Vec::new(),
                latency_us: start.elapsed().as_micros() as u64,
                rationale_present: false,
            };
        }

        let mut ranked: Vec<ScoredPrediction> = scores
            .into_iter()
            .map(|(package, score)| ScoredPrediction { package, score })
            .collect();
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    let ta = latest_ts.get(&a.package).copied().unwrap_or(i64::MIN);
                    let tb = latest_ts.get(&b.package).copied().unwrap_or(i64::MIN);
                    tb.cmp(&ta)
                })
                .then_with(|| a.package.cmp(&b.package))
        });

        PredictionResult {
            ranked,
            latency_us: start.elapsed().as_micros() as u64,
            rationale_present: false,
        }
    }
}

/// Prewarm the app most recently switched to (proxied by last foreground target).
pub struct LastAppPrewarmBackend;

impl NextAppPredictor for LastAppPrewarmBackend {
    fn name(&self) -> &'static str {
        "last_app_prewarm"
    }

    fn predict(
        &self,
        ctx: &aios_spec::StructuredContext,
        current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let start = Instant::now();
        PredictionResult {
            ranked: last_foreground_ranked(ctx, current_app, candidates),
            latency_us: start.elapsed().as_micros() as u64,
            rationale_present: false,
        }
    }
}
