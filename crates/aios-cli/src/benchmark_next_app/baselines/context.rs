//! Context-aware baseline predictors for the next-app benchmark.

use std::time::Instant;

use aios_spec::{AppTransition, SanitizedEventType};

use crate::benchmark_next_app::types::{NextAppPredictor, PredictionResult, ScoredPrediction};

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
    use aios_spec::{
        AppTransition, ContextSummary, SanitizedEvent, SanitizedEventType, SourceTier,
        StructuredContext,
    };

    use super::*;

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
    fn last_non_current_foreground_tie_breaks_alphabetically() {
        let ctx = ctx_with_events(vec![
            foreground_event(100, "B"),
            foreground_event(100, "A"),
            foreground_event(200, "C"),
        ]);
        let result = last_non_current_foreground(&ctx, "C");
        assert_eq!(result, Some((100, "A".into())));
    }
}
