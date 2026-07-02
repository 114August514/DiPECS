//! Simple baseline predictors for the next-app benchmark.

use std::cell::RefCell;
use std::cmp::Ordering;
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

fn rank_by_counts(candidates: &[String], counts: &HashMap<String, u32>) -> Vec<ScoredPrediction> {
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

/// Compare scored predictions by descending score.
/// Callers add their own tie-breakers for determinism.
fn cmp_score_desc(a: &ScoredPrediction, b: &ScoredPrediction) -> Ordering {
    b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal)
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
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
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
            .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
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

/// True for categories that launcher-style ranking treats as time-critical.
fn is_priority_category(category: &Option<String>) -> bool {
    category.as_ref().is_some_and(|cat| {
        cat.eq_ignore_ascii_case("alarm")
            || cat.eq_ignore_ascii_case("call")
            || cat.eq_ignore_ascii_case("event")
    })
}

/// Score a single notification event for priority ranking.
///
/// Weights mirror launcher-style notification priority:
/// - ongoing notifications (+3) are persistent and high-visibility.
/// - rich attachments / mentions (+2 for file/image/link) signal actionable content.
/// - social/calendar signals and system categories (+1 each) are weaker but still salient.
/// - the most recent notification timestamp (+1) gives recency bias.
fn score_notification(
    category: &Option<String>,
    is_ongoing: bool,
    semantic_hints: &[SemanticHint],
    is_most_recent: bool,
) -> f32 {
    let mut score = 0.0;
    if is_ongoing {
        score += 3.0;
    }
    for hint in semantic_hints {
        match hint {
            SemanticHint::FileMention
            | SemanticHint::ImageMention
            | SemanticHint::LinkAttachment => score += 2.0,
            SemanticHint::UserMentioned | SemanticHint::CalendarInvitation => score += 1.0,
            _ => {},
        }
    }
    if is_priority_category(category) {
        score += 1.0;
    }
    if is_most_recent {
        score += 1.0;
    }
    score
}

/// Sort by score descending, then by most recent timestamp descending,
/// and finally by package name ascending for determinism.
fn rank_by_priority(
    mut scored: Vec<ScoredPrediction>,
    latest_ts: &HashMap<String, i64>,
) -> Vec<ScoredPrediction> {
    scored.sort_by(|a, b| {
        cmp_score_desc(a, b)
            .then_with(|| {
                let ta = latest_ts.get(&a.package).copied().unwrap_or(i64::MIN);
                let tb = latest_ts.get(&b.package).copied().unwrap_or(i64::MIN);
                tb.cmp(&ta)
            })
            .then_with(|| a.package.cmp(&b.package))
    });
    scored
}

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
            .expect("notifications are non-empty");

        let mut scores: HashMap<String, f32> = HashMap::new();
        let mut latest_ts: HashMap<String, i64> = HashMap::new();

        for event in &notifications {
            if let SanitizedEventType::Notification {
                source_package,
                category,
                is_ongoing,
                semantic_hints,
                ..
            } = &event.event_type
            {
                if !candidate_set.contains(source_package) {
                    continue;
                }

                latest_ts
                    .entry(source_package.clone())
                    .and_modify(|v| *v = (*v).max(event.timestamp_ms))
                    .or_insert(event.timestamp_ms);

                let delta = score_notification(
                    category,
                    *is_ongoing,
                    semantic_hints,
                    event.timestamp_ms == max_ts,
                );
                *scores.entry(source_package.clone()).or_insert(0.0) += delta;
            }
        }

        if scores.is_empty() {
            return PredictionResult {
                ranked: Vec::new(),
                latency_us: start.elapsed().as_micros() as u64,
                rationale_present: false,
            };
        }

        let scored: Vec<ScoredPrediction> = scores
            .into_iter()
            .map(|(package, score)| ScoredPrediction { package, score })
            .collect();
        let ranked = rank_by_priority(scored, &latest_ts);

        PredictionResult {
            ranked,
            latency_us: start.elapsed().as_micros() as u64,
            rationale_present: false,
        }
    }
}

/// Prewarm the app most recently switched to.
///
/// Synthetic traces do not carry explicit "was prewarmed" action history, so this
/// backend intentionally proxies the same signal as `LastForegroundBackend`: the
/// most recent non-current foreground target. This keeps the baseline realistic
/// and deterministic while remaining trivial to upgrade once traces expose real
/// prewarm feedback.
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

#[cfg(test)]
mod tests {
    use super::*;
    use aios_spec::{
        AppTransition, ContextSummary, SanitizedEvent, SanitizedEventType, ScriptHint,
        SemanticHint, SourceTier, StructuredContext, TextHint,
    };

    fn text_hint() -> TextHint {
        TextHint {
            length_chars: 0,
            script: ScriptHint::Unknown,
            is_emoji_only: false,
        }
    }

    fn ctx_with_events(events: Vec<SanitizedEvent>) -> StructuredContext {
        StructuredContext {
            window_id: "w1".into(),
            window_start_ms: 0,
            window_end_ms: 10_000,
            duration_secs: 10,
            events,
            summary: ContextSummary {
                foreground_apps: vec![],
                notified_apps: vec![],
                all_semantic_hints: vec![],
                file_activity: vec![],
                latest_system_status: None,
                source_tier: SourceTier::PublicApi,
            },
        }
    }

    fn notification_event(
        timestamp_ms: i64,
        source_package: &str,
        category: Option<&str>,
        is_ongoing: bool,
        semantic_hints: Vec<SemanticHint>,
    ) -> SanitizedEvent {
        SanitizedEvent {
            event_id: format!("n{timestamp_ms}"),
            timestamp_ms,
            event_type: SanitizedEventType::Notification {
                source_package: source_package.into(),
                category: category.map(Into::into),
                channel_id: None,
                title_hint: text_hint(),
                text_hint: text_hint(),
                semantic_hints,
                is_ongoing,
                group_key: None,
            },
            source_tier: SourceTier::PublicApi,
            app_package: Some(source_package.into()),
            uid: None,
        }
    }

    fn foreground_event(timestamp_ms: i64, package_name: &str) -> SanitizedEvent {
        SanitizedEvent {
            event_id: format!("fg{timestamp_ms}"),
            timestamp_ms,
            event_type: SanitizedEventType::AppTransition {
                package_name: package_name.into(),
                activity_class: None,
                transition: AppTransition::Foreground,
            },
            source_tier: SourceTier::PublicApi,
            app_package: Some(package_name.into()),
            uid: None,
        }
    }

    #[test]
    fn recent_notification_is_noop_for_empty_context() {
        let backend = RecentNotificationBackend;
        let result = backend.predict(&ctx_with_events(vec![]), "A", &["B".into(), "C".into()]);
        assert!(result.ranked.is_empty());
    }

    #[test]
    fn recent_notification_noop_when_source_not_in_candidates() {
        let ctx = ctx_with_events(vec![notification_event(100, "B", None, false, vec![])]);
        let backend = RecentNotificationBackend;
        let result = backend.predict(&ctx, "A", &["C".into()]);
        assert!(result.ranked.is_empty());
    }

    #[test]
    fn recent_notification_picks_most_recent_in_candidates() {
        let ctx = ctx_with_events(vec![
            notification_event(100, "B", None, false, vec![]),
            notification_event(200, "C", None, false, vec![]),
        ]);
        let backend = RecentNotificationBackend;
        let result = backend.predict(&ctx, "A", &["B".into(), "C".into()]);
        assert_eq!(result.ranked.len(), 1);
        assert_eq!(result.ranked[0].package, "C");
        assert_eq!(result.ranked[0].score, 1.0);
    }

    #[test]
    fn recent_notification_tie_breaks_alphabetically() {
        let ctx = ctx_with_events(vec![
            notification_event(100, "B", None, false, vec![]),
            notification_event(100, "A", None, false, vec![]),
        ]);
        let backend = RecentNotificationBackend;
        let result = backend.predict(&ctx, "Z", &["A".into(), "B".into()]);
        assert_eq!(result.ranked[0].package, "A");
    }

    #[test]
    fn last_foreground_is_noop_for_empty_context() {
        let backend = LastForegroundBackend;
        let result = backend.predict(&ctx_with_events(vec![]), "A", &["B".into()]);
        assert!(result.ranked.is_empty());
    }

    #[test]
    fn last_foreground_excludes_current_app() {
        let ctx = ctx_with_events(vec![foreground_event(200, "A"), foreground_event(100, "B")]);
        let backend = LastForegroundBackend;
        let result = backend.predict(&ctx, "A", &["B".into(), "C".into()]);
        assert_eq!(result.ranked.len(), 1);
        assert_eq!(result.ranked[0].package, "B");
        assert_eq!(result.ranked[0].score, 1.0);
    }

    #[test]
    fn last_foreground_noop_when_target_not_candidate() {
        let ctx = ctx_with_events(vec![foreground_event(100, "B")]);
        let backend = LastForegroundBackend;
        let result = backend.predict(&ctx, "A", &["C".into()]);
        assert!(result.ranked.is_empty());
    }

    #[test]
    fn last_app_prewarm_uses_last_foreground_proxy() {
        let ctx = ctx_with_events(vec![foreground_event(200, "A"), foreground_event(100, "B")]);
        let prewarm = LastAppPrewarmBackend;
        let foreground = LastForegroundBackend;
        assert_eq!(
            prewarm.predict(&ctx, "A", &["B".into()]).ranked,
            foreground.predict(&ctx, "A", &["B".into()]).ranked
        );
    }

    #[test]
    fn notification_priority_is_noop_without_notifications() {
        let backend = NotificationPriorityBackend;
        let result = backend.predict(&ctx_with_events(vec![]), "A", &["B".into()]);
        assert!(result.ranked.is_empty());
    }

    #[test]
    fn notification_priority_is_noop_when_no_candidates_match() {
        let ctx = ctx_with_events(vec![notification_event(100, "B", None, false, vec![])]);
        let backend = NotificationPriorityBackend;
        let result = backend.predict(&ctx, "A", &["C".into()]);
        assert!(result.ranked.is_empty());
    }

    #[test]
    fn notification_priority_applies_weights_and_ranks() {
        let ctx = ctx_with_events(vec![
            // A: file mention = 2 points.
            notification_event(100, "A", None, false, vec![SemanticHint::FileMention]),
            // B: ongoing + alarm + user mention + most recent = 3 + 1 + 1 + 1 = 6.
            notification_event(
                200,
                "B",
                Some("alarm"),
                true,
                vec![SemanticHint::UserMentioned],
            ),
        ]);
        let backend = NotificationPriorityBackend;
        let result = backend.predict(&ctx, "Z", &["A".into(), "B".into()]);
        assert_eq!(result.ranked.len(), 2);
        assert_eq!(result.ranked[0].package, "B");
        assert_eq!(result.ranked[0].score, 6.0);
        assert_eq!(result.ranked[1].package, "A");
        assert_eq!(result.ranked[1].score, 2.0);
    }

    #[test]
    fn notification_priority_tie_breaks_by_timestamp_then_name() {
        // A and B end with the same total score (5), but A is more recent.
        let ctx = ctx_with_events(vec![
            notification_event(100, "B", None, true, vec![SemanticHint::ImageMention]), // 3+2 = 5
            notification_event(200, "A", None, true, vec![SemanticHint::UserMentioned]), // 3+1+1(most recent) = 5
        ]);
        let backend = NotificationPriorityBackend;
        let result = backend.predict(&ctx, "Z", &["A".into(), "B".into()]);
        assert_eq!(result.ranked[0].package, "A");
        assert_eq!(result.ranked[1].package, "B");

        // Same score and timestamp: alphabetical tie-break.
        let ctx_tied = ctx_with_events(vec![
            notification_event(100, "B", None, true, vec![SemanticHint::UserMentioned]), // 3+1+1(most recent) = 5
            notification_event(100, "A", None, true, vec![SemanticHint::UserMentioned]), // 3+1+1(most recent) = 5
        ]);
        let result_tied = backend.predict(&ctx_tied, "Z", &["A".into(), "B".into()]);
        assert_eq!(result_tied.ranked[0].package, "A");
        assert_eq!(result_tied.ranked[1].package, "B");
    }

    #[test]
    fn last_non_current_foreground_tie_breaks_alphabetically() {
        let ctx = ctx_with_events(vec![
            foreground_event(100, "B"),
            foreground_event(100, "A"),
            foreground_event(200, "C"),
        ]);
        let result = last_non_current_foreground(&ctx, "C");
        assert_eq!(result, Some((100, "A".into())));
    }

    #[test]
    fn is_priority_category_is_case_insensitive() {
        assert!(is_priority_category(&Some("Alarm".into())));
        assert!(is_priority_category(&Some("CALL".into())));
        assert!(is_priority_category(&Some("event".into())));
        assert!(!is_priority_category(&Some("msg".into())));
        assert!(!is_priority_category(&None));
    }
}
