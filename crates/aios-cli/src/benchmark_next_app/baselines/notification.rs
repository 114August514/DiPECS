//! Notification-based baseline predictors for the next-app benchmark.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use aios_spec::{SanitizedEventType, SemanticHint};

use crate::benchmark_next_app::types::{NextAppPredictor, PredictionResult, ScoredPrediction};

use super::simple::cmp_score_desc;

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

#[cfg(test)]
mod tests {
    use aios_spec::{
        ContextSummary, SanitizedEvent, SanitizedEventType, ScriptHint, SemanticHint, SourceTier,
        StructuredContext, TextHint,
    };

    use super::*;

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
    fn is_priority_category_is_case_insensitive() {
        assert!(is_priority_category(&Some("Alarm".into())));
        assert!(is_priority_category(&Some("CALL".into())));
        assert!(is_priority_category(&Some("event".into())));
        assert!(!is_priority_category(&Some("msg".into())));
        assert!(!is_priority_category(&None));
    }
}
