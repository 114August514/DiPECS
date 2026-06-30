//! DecisionRouter - multi-tier decision routing.
//!
//! Routing priority:
//! 1. Circuit breaker: too many consecutive backend errors -> FallbackNoOp.
//! 2. Privacy sensitivity: too many sensitive signals -> RuleBased.
//! 3. Semantic complexity: low complexity -> RuleBased, medium/high ->
//!    CloudLlm when configured, otherwise LocalEvaluator.
use std::cell::RefCell;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use aios_spec::{
    DecisionBackendResult, DecisionRoute, ModelInput, SanitizedEventType, SemanticHint,
    StructuredContext,
};

use crate::backends::cloud_llm::CloudBackendState;
use crate::backends::fallback::FallbackNoOpBackend;
use crate::backends::local_evaluator::LocalEvaluatorBackend;
use crate::backends::rule_based::RuleBasedBackend;
use crate::DecisionBackend;

// ============================================================
// RouterConfig
// ============================================================

#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Number of privacy-sensitive signals above which cloud routing is blocked.
    pub privacy_score_threshold: usize,
    /// Number of consecutive errors before the circuit breaker trips.
    pub circuit_breaker_threshold: u32,
    /// Time window (in seconds) over which consecutive errors are counted.
    pub circuit_breaker_window_secs: u64,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            privacy_score_threshold: 3,
            circuit_breaker_threshold: 5,
            circuit_breaker_window_secs: 60,
        }
    }
}

// ============================================================
// Circuit breaker state
// ============================================================

#[derive(Debug, Clone)]
struct ErrorRecord {
    timestamp: Instant,
}

#[derive(Debug, Clone, Default)]
struct CircuitState {
    errors: Vec<ErrorRecord>,
}

impl CircuitState {
    fn record_error(&mut self) {
        self.errors.push(ErrorRecord {
            timestamp: Instant::now(),
        });
    }

    fn record_success(&mut self) {
        self.errors.clear();
    }

    fn count_recent_errors(&self, window_secs: u64) -> u32 {
        let cutoff = Instant::now()
            .checked_sub(Duration::from_secs(window_secs))
            .unwrap_or(Instant::now());
        self.errors.iter().filter(|e| e.timestamp >= cutoff).count() as u32
    }
}

// ============================================================
// Routing reason
// ============================================================

#[derive(Debug, Clone)]
enum RoutingReason {
    CircuitBreakerTripped { failure_count: u32 },
    PrivacySensitive { score: usize },
    LocalActionableSignal,
    LowComplexity,
    LocalPreferred { complexity: &'static str },
    CloudPreferred { complexity: &'static str },
}

impl RoutingReason {
    fn tag(&self) -> String {
        match self {
            RoutingReason::CircuitBreakerTripped { failure_count } => {
                format!("routing:circuit_breaker_fallback(errors={failure_count})")
            },
            RoutingReason::PrivacySensitive { score } => {
                format!("routing:privacy_sensitive(score={score})")
            },
            RoutingReason::LocalActionableSignal => "routing:local_actionable_signal".into(),
            RoutingReason::LowComplexity => "routing:low_complexity".into(),
            RoutingReason::LocalPreferred { complexity } => {
                format!("routing:{complexity}_complexity(local_evaluator)")
            },
            RoutingReason::CloudPreferred { complexity } => {
                format!("routing:{complexity}_complexity(cloud_llm)")
            },
        }
    }
}

// ============================================================
// DecisionRouter
// ============================================================

pub struct DecisionRouter {
    config: RouterConfig,
    rule_based: Box<dyn DecisionBackend + Send + Sync>,
    local_evaluator: Box<dyn DecisionBackend + Send + Sync>,
    fallback: Box<dyn DecisionBackend + Send + Sync>,
    cloud_llm: Option<Box<dyn DecisionBackend + Send + Sync>>,
    cloud_disabled: bool,
    cloud_misconfigured: Option<String>,
    circuit_state: RefCell<CircuitState>,
}

impl DecisionRouter {
    pub fn new(config: RouterConfig) -> Self {
        let cloud_state = CloudBackendState::from_env();
        if let CloudBackendState::Misconfigured(error) = &cloud_state {
            tracing::warn!(
                error = %error,
                "cloud llm backend configuration ignored; DecisionRouter will stay local"
            );
        }

        let (cloud_llm, cloud_disabled, cloud_misconfigured) = match cloud_state {
            CloudBackendState::Ready(backend) => {
                let backend: Box<dyn DecisionBackend + Send + Sync> = Box::new(backend);
                (Some(backend), false, None)
            },
            CloudBackendState::Disabled => (None, true, None),
            CloudBackendState::Misconfigured(error) => (None, false, Some(error)),
        };

        Self {
            config,
            rule_based: Box::new(RuleBasedBackend),
            local_evaluator: Box::new(LocalEvaluatorBackend),
            fallback: Box::new(FallbackNoOpBackend),
            cloud_llm,
            cloud_disabled,
            cloud_misconfigured,
            circuit_state: RefCell::new(CircuitState::default()),
        }
    }

    #[cfg(test)]
    fn with_backends(
        config: RouterConfig,
        rule_based: Box<dyn DecisionBackend + Send + Sync>,
        local_evaluator: Box<dyn DecisionBackend + Send + Sync>,
        cloud_llm: Option<Box<dyn DecisionBackend + Send + Sync>>,
        fallback: Box<dyn DecisionBackend + Send + Sync>,
    ) -> Self {
        let cloud_disabled = cloud_llm.is_none();
        Self {
            config,
            rule_based,
            local_evaluator,
            fallback,
            cloud_llm,
            cloud_disabled,
            cloud_misconfigured: None,
            circuit_state: RefCell::new(CircuitState::default()),
        }
    }

    /// Evaluate a StructuredContext through the routing pipeline.
    ///
    /// Uses interior mutability (`RefCell`) to track circuit breaker state
    /// across calls without requiring `&mut self`.
    pub fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        let input = ModelInput::current_only(context.clone());
        self.evaluate_model_input(&input)
    }

    /// Evaluate a model input that includes the current window plus optional
    /// behavior profile and recent feedback memory.
    pub fn evaluate_model_input(&self, input: &ModelInput) -> DecisionBackendResult {
        let context = &input.current_context;
        let (route, reason) = self.determine_route(context);
        let reason_tag = reason.tag();

        let (mut result, backend_failed) = match route {
            DecisionRoute::RuleBased => {
                let result = self.rule_based.evaluate_model_input(input);
                let backend_failed = result.error.is_some();
                (result, backend_failed)
            },
            DecisionRoute::LocalEvaluator => {
                let result = self.local_evaluator.evaluate_model_input(input);
                let backend_failed = result.error.is_some();
                (result, backend_failed)
            },
            DecisionRoute::CloudLlm => match &self.cloud_llm {
                Some(backend) => {
                    let cloud_result = backend.evaluate_model_input(input);
                    if let Some(error) = cloud_result.error.as_deref() {
                        let mut fallback = self.rule_based.evaluate_model_input(input);
                        fallback.error = Some(format!("cloud llm backend failed: {error}"));
                        fallback
                            .rationale_tags
                            .push("backend:cloud_llm_error(rule_based_fallback)".to_string());
                        (fallback, true)
                    } else {
                        (cloud_result, false)
                    }
                },
                None => {
                    let result = self.rule_based.evaluate_model_input(input);
                    let backend_failed = result.error.is_some();
                    (result, backend_failed)
                },
            },
            DecisionRoute::FallbackNoOp => {
                let result = self.fallback.evaluate_model_input(input);
                // FallbackNoOp may preserve an audit error while successfully
                // generating the safe NoOp used to probe recovery.
                (result, false)
            },
            _ => {
                let result = self.rule_based.evaluate_model_input(input);
                let backend_failed = result.error.is_some();
                (result, backend_failed)
            },
        };

        // Inject routing reason tag
        if !result.rationale_tags.iter().any(|tag| tag == &reason_tag) {
            result.rationale_tags.push(reason_tag);
        }

        // Update circuit breaker state
        let mut state = self.circuit_state.borrow_mut();
        if backend_failed {
            state.record_error();
        } else {
            state.record_success();
        }

        result
    }

    // --- Private routing logic ---

    fn determine_route(&self, context: &StructuredContext) -> (DecisionRoute, RoutingReason) {
        // Priority 1: Circuit breaker
        let error_count = self
            .circuit_state
            .borrow()
            .count_recent_errors(self.config.circuit_breaker_window_secs);
        if error_count >= self.config.circuit_breaker_threshold {
            return (
                DecisionRoute::FallbackNoOp,
                RoutingReason::CircuitBreakerTripped {
                    failure_count: error_count,
                },
            );
        }

        // Priority 2: Privacy sensitivity
        let privacy_score = Self::compute_privacy_score(context);
        if privacy_score > self.config.privacy_score_threshold {
            return (
                DecisionRoute::RuleBased,
                RoutingReason::PrivacySensitive {
                    score: privacy_score,
                },
            );
        }

        // LocalEvaluator owns low-risk proactive actions such as prefetch,
        // process prewarm, and work-scoped keepalive. Route these signals to it
        // before the generic semantic-complexity split so single-hint or
        // FileActivity-only windows are not trapped in RuleBased.
        if Self::has_local_actionable_signal(context) {
            return (
                DecisionRoute::LocalEvaluator,
                RoutingReason::LocalActionableSignal,
            );
        }

        // Priority 3: Semantic complexity
        let unique_types = Self::count_unique_semantic_hint_types(context);
        match unique_types {
            0 | 1 => (DecisionRoute::RuleBased, RoutingReason::LowComplexity),
            2 | 3 => self.cloud_route_or_fallback("medium"),
            _ => self.cloud_route_or_fallback("high"),
        }
    }

    fn cloud_route_or_fallback(&self, complexity: &'static str) -> (DecisionRoute, RoutingReason) {
        if self.cloud_llm.is_some() {
            return (
                DecisionRoute::CloudLlm,
                RoutingReason::CloudPreferred { complexity },
            );
        }
        if self.cloud_disabled || self.cloud_misconfigured.is_some() {
            return (
                DecisionRoute::LocalEvaluator,
                RoutingReason::LocalPreferred { complexity },
            );
        }
        // No cloud backend available; stay local for safety.
        (
            DecisionRoute::LocalEvaluator,
            RoutingReason::LocalPreferred { complexity },
        )
    }

    /// Count privacy-sensitive signals:
    /// - Notification events with VerificationCode or FinancialContext hints
    /// - AppTransition events
    fn compute_privacy_score(context: &StructuredContext) -> usize {
        context
            .events
            .iter()
            .map(|event| match &event.event_type {
                SanitizedEventType::Notification { semantic_hints, .. } => semantic_hints
                    .iter()
                    .filter(|h| {
                        matches!(
                            h,
                            SemanticHint::VerificationCode | SemanticHint::FinancialContext
                        )
                    })
                    .count(),
                SanitizedEventType::AppTransition { .. } => 1,
                _ => 0,
            })
            .sum()
    }

    fn has_local_actionable_signal(context: &StructuredContext) -> bool {
        context.events.iter().any(|event| match &event.event_type {
            SanitizedEventType::FileActivity { .. } => true,
            SanitizedEventType::Notification { semantic_hints, .. } => {
                semantic_hints.iter().any(|hint| {
                    matches!(
                        hint,
                        SemanticHint::FileMention
                            | SemanticHint::ImageMention
                            | SemanticHint::LinkAttachment
                    )
                })
            },
            SanitizedEventType::AppTransition {
                transition: aios_spec::AppTransition::Foreground,
                ..
            } => true,
            _ => false,
        })
    }

    /// Count unique SemanticHint variants across all notification events.
    fn count_unique_semantic_hint_types(context: &StructuredContext) -> usize {
        let mut seen: HashSet<&SemanticHint> = HashSet::new();
        for event in &context.events {
            if let SanitizedEventType::Notification { semantic_hints, .. } = &event.event_type {
                for hint in semantic_hints {
                    seen.insert(hint);
                }
            }
        }
        seen.len()
    }
}

impl Default for DecisionRouter {
    fn default() -> Self {
        Self::new(RouterConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aios_spec::{
        ActionType, ActionUrgency, ContextSummary, DecisionBackendResult, DecisionRoute, Intent,
        IntentBatch, IntentType, RiskLevel, SanitizedEvent, SanitizedEventType, SemanticHint,
        SourceTier, StructuredContext, SuggestedAction,
    };

    use crate::DecisionBackend;

    fn empty_context() -> StructuredContext {
        StructuredContext {
            window_id: "test-window".into(),
            window_start_ms: 0,
            window_end_ms: 1000,
            duration_secs: 1,
            events: vec![],
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

    fn idle_batch(context: &StructuredContext) -> IntentBatch {
        IntentBatch {
            window_id: context.window_id.clone(),
            intents: vec![Intent {
                intent_id: "idle".into(),
                intent_type: IntentType::Idle,
                confidence: 1.0,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::NoOp,
                    target: None,
                    urgency: ActionUrgency::IdleTime,
                }],
                rationale_tags: vec![],
            }],
            generated_at_ms: context.window_end_ms,
            model: "test".into(),
        }
    }

    /// A backend that always fails, carrying the given route label and error message.
    struct FailingBackend {
        route: DecisionRoute,
        error: String,
    }

    impl DecisionBackend for FailingBackend {
        fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
            DecisionBackendResult {
                route: self.route,
                intent_batch: idle_batch(context),
                rationale_tags: vec!["failing_backend".into()],
                latency_us: 0,
                error: Some(self.error.clone()),
            }
        }
    }

    /// A backend that always succeeds, carrying the given route label.
    struct OkBackend {
        route: DecisionRoute,
    }

    impl DecisionBackend for OkBackend {
        fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
            DecisionBackendResult {
                route: self.route,
                intent_batch: idle_batch(context),
                rationale_tags: vec!["ok_backend".into()],
                latency_us: 0,
                error: None,
            }
        }
    }

    #[test]
    fn circuit_state_persists_across_evaluate_calls() {
        let config = RouterConfig {
            circuit_breaker_threshold: 2,
            circuit_breaker_window_secs: 3600,
            ..RouterConfig::default()
        };
        let router = DecisionRouter::with_backends(
            config,
            Box::new(FailingBackend {
                route: DecisionRoute::RuleBased,
                error: "rule-based failure".into(),
            }),
            Box::new(OkBackend {
                route: DecisionRoute::LocalEvaluator,
            }),
            None,
            Box::new(FallbackNoOpBackend),
        );
        let ctx = empty_context();

        // First window: one error recorded, circuit still closed.
        let r1 = router.evaluate(&ctx);
        assert!(
            !matches!(r1.route, DecisionRoute::FallbackNoOp),
            "first failure should not trip breaker"
        );

        // Second window: second error pushes the counter over the threshold.
        let r2 = router.evaluate(&ctx);
        assert!(
            !matches!(r2.route, DecisionRoute::FallbackNoOp),
            "route is determined before the second failure is recorded"
        );

        // Third window: circuit is now open, so we fall back to NoOp.
        let r3 = router.evaluate(&ctx);
        let route = &r3.route;
        assert!(
            matches!(r3.route, DecisionRoute::FallbackNoOp),
            "circuit breaker should trip after two consecutive errors, got {route:?}"
        );
    }

    #[test]
    fn circuit_state_resets_after_successful_fallback() {
        let config = RouterConfig {
            circuit_breaker_threshold: 2,
            circuit_breaker_window_secs: 3600,
            ..RouterConfig::default()
        };
        let router = DecisionRouter::with_backends(
            config,
            Box::new(FailingBackend {
                route: DecisionRoute::RuleBased,
                error: "rule-based failure".into(),
            }),
            Box::new(OkBackend {
                route: DecisionRoute::LocalEvaluator,
            }),
            None,
            Box::new(FallbackNoOpBackend),
        );
        let ctx = empty_context();

        // Trip the breaker with two consecutive failures.
        let _ = router.evaluate(&ctx);
        let _ = router.evaluate(&ctx);
        let r_open = router.evaluate(&ctx);
        assert!(
            matches!(r_open.route, DecisionRoute::FallbackNoOp),
            "breaker should be open"
        );
        assert!(
            r_open.error.is_some(),
            "real fallback should preserve an audit error while succeeding safely"
        );

        // A generated NoOp is a successful safe fallback, even though it preserves
        // an audit error for downstream visibility.
        let r_reset = router.evaluate(&ctx);
        let route = &r_reset.route;
        assert!(
            !matches!(r_reset.route, DecisionRoute::FallbackNoOp),
            "circuit should reset after a successful fallback, got {route:?}"
        );
    }

    #[test]
    fn circuit_state_counts_cloud_backend_errors() {
        // Use a context that routes to CloudLlm: two distinct semantic hint
        // types and a low privacy score.
        let ctx = StructuredContext {
            window_id: "cloud-route-window".into(),
            window_start_ms: 0,
            window_end_ms: 1000,
            duration_secs: 1,
            events: vec![
                SanitizedEvent {
                    event_id: "n1".into(),
                    timestamp_ms: 100,
                    event_type: SanitizedEventType::Notification {
                        source_package: "com.a".into(),
                        category: None,
                        channel_id: None,
                        title_hint: aios_spec::TextHint {
                            length_chars: 1,
                            script: aios_spec::ScriptHint::Latin,
                            is_emoji_only: false,
                        },
                        text_hint: aios_spec::TextHint {
                            length_chars: 1,
                            script: aios_spec::ScriptHint::Latin,
                            is_emoji_only: false,
                        },
                        semantic_hints: vec![SemanticHint::UserMentioned],
                        is_ongoing: false,
                        group_key: None,
                    },
                    source_tier: SourceTier::PublicApi,
                    app_package: None,
                    uid: None,
                },
                SanitizedEvent {
                    event_id: "n2".into(),
                    timestamp_ms: 200,
                    event_type: SanitizedEventType::Notification {
                        source_package: "com.b".into(),
                        category: None,
                        channel_id: None,
                        title_hint: aios_spec::TextHint {
                            length_chars: 1,
                            script: aios_spec::ScriptHint::Latin,
                            is_emoji_only: false,
                        },
                        text_hint: aios_spec::TextHint {
                            length_chars: 1,
                            script: aios_spec::ScriptHint::Latin,
                            is_emoji_only: false,
                        },
                        semantic_hints: vec![SemanticHint::CalendarInvitation],
                        is_ongoing: false,
                        group_key: None,
                    },
                    source_tier: SourceTier::PublicApi,
                    app_package: None,
                    uid: None,
                },
            ],
            summary: ContextSummary {
                foreground_apps: vec![],
                notified_apps: vec!["com.a".into(), "com.b".into()],
                all_semantic_hints: vec![
                    SemanticHint::UserMentioned,
                    SemanticHint::CalendarInvitation,
                ],
                file_activity: vec![],
                latest_system_status: None,
                source_tier: SourceTier::PublicApi,
            },
        };

        let config = RouterConfig {
            privacy_score_threshold: 10,
            circuit_breaker_threshold: 2,
            circuit_breaker_window_secs: 3600,
        };
        let router = DecisionRouter::with_backends(
            config,
            Box::new(OkBackend {
                route: DecisionRoute::RuleBased,
            }),
            Box::new(OkBackend {
                route: DecisionRoute::LocalEvaluator,
            }),
            Some(Box::new(FailingBackend {
                route: DecisionRoute::CloudLlm,
                error: "cloud failure".into(),
            })),
            Box::new(FallbackNoOpBackend),
        );

        let r1 = router.evaluate(&ctx);
        assert!(
            matches!(r1.route, DecisionRoute::RuleBased),
            "cloud failure falls back to rule-based before the circuit trips"
        );
        assert!(
            r1.error.as_deref().unwrap_or("").contains("cloud failure"),
            "cloud error should be preserved in the fallback result"
        );

        let r2 = router.evaluate(&ctx);
        assert!(
            matches!(r2.route, DecisionRoute::RuleBased),
            "second cloud failure still routes through rule-based fallback"
        );

        let r3 = router.evaluate(&ctx);
        let route = &r3.route;
        assert!(
            matches!(r3.route, DecisionRoute::FallbackNoOp),
            "cloud errors should trip the circuit breaker, got {route:?}"
        );
    }
    #[test]
    fn cloud_disabled_medium_complexity_routes_to_local_evaluator() {
        let ctx = StructuredContext {
            window_id: "local-route-window".into(),
            window_start_ms: 0,
            window_end_ms: 1000,
            duration_secs: 1,
            events: vec![SanitizedEvent {
                event_id: "n1".into(),
                timestamp_ms: 100,
                event_type: SanitizedEventType::Notification {
                    source_package: "com.chat".into(),
                    category: None,
                    channel_id: None,
                    title_hint: aios_spec::TextHint {
                        length_chars: 1,
                        script: aios_spec::ScriptHint::Latin,
                        is_emoji_only: false,
                    },
                    text_hint: aios_spec::TextHint {
                        length_chars: 1,
                        script: aios_spec::ScriptHint::Latin,
                        is_emoji_only: false,
                    },
                    semantic_hints: vec![
                        SemanticHint::UserMentioned,
                        SemanticHint::CalendarInvitation,
                    ],
                    is_ongoing: false,
                    group_key: None,
                },
                source_tier: SourceTier::PublicApi,
                app_package: Some("com.chat".into()),
                uid: None,
            }],
            summary: ContextSummary {
                foreground_apps: vec![],
                notified_apps: vec!["com.chat".into()],
                all_semantic_hints: vec![
                    SemanticHint::UserMentioned,
                    SemanticHint::CalendarInvitation,
                ],
                file_activity: vec![],
                latest_system_status: None,
                source_tier: SourceTier::PublicApi,
            },
        };

        let router = DecisionRouter::with_backends(
            RouterConfig::default(),
            Box::new(OkBackend {
                route: DecisionRoute::RuleBased,
            }),
            Box::new(OkBackend {
                route: DecisionRoute::LocalEvaluator,
            }),
            None,
            Box::new(FallbackNoOpBackend),
        );

        let result = router.evaluate(&ctx);
        assert!(matches!(result.route, DecisionRoute::LocalEvaluator));
        assert!(result
            .rationale_tags
            .iter()
            .any(|tag| tag == "routing:medium_complexity(local_evaluator)"));
    }
}
