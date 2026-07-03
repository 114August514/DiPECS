//! DecisionRouter - multi-tier decision routing.
//!
//! Routing priority:
//! 1. Circuit breaker: too many consecutive backend errors -> FallbackNoOp.
//! 2. Privacy sensitivity: notifications with verification/financial hints ->
//!    RuleBased (blocks cloud routing while keeping sensitive data local).
//!    App transitions carry no raw text and are excluded from the privacy score,
//!    so they pass through this gate unchanged.
//! 3. Local actionable signal: foreground transitions, file activity, actionable
//!    notifications -> LocalEvaluator (so next-app prediction and other local
//!    proactive actions are not blocked by the privacy gate).
//! 4. Semantic complexity: low complexity -> RuleBased, medium/high -> CloudLlm
//!    when configured, otherwise LocalEvaluator.
use std::cell::RefCell;
use std::collections::HashSet;
#[cfg(test)]
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aios_spec::{
    ActionType, DecisionBackendResult, DecisionRoute, Intent, IntentBatch, IntentType, ModelInput,
    SanitizedEventType, SemanticHint, StructuredContext,
};

use crate::backends::cloud_llm::CloudBackendState;
use crate::backends::fallback::FallbackNoOpBackend;
use crate::backends::local_evaluator::LocalEvaluatorBackend;
use crate::backends::predictive::PredictiveLocalBackend;
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

struct CompositeLocalEvaluatorBackend {
    heuristic: LocalEvaluatorBackend,
    predictive: PredictiveLocalBackend,
}

impl CompositeLocalEvaluatorBackend {
    fn new(predictive: PredictiveLocalBackend) -> Self {
        Self {
            heuristic: LocalEvaluatorBackend,
            predictive,
        }
    }

    fn merge_results(
        &self,
        input: &ModelInput,
        heuristic: DecisionBackendResult,
        predictive: DecisionBackendResult,
    ) -> DecisionBackendResult {
        let mut heuristic_intents = heuristic.intent_batch.intents;
        let mut predictive_intents = predictive.intent_batch.intents;
        let heuristic_idle = take_idle_noop(&mut heuristic_intents);
        let predictive_idle = take_idle_noop(&mut predictive_intents);

        let mut intents = heuristic_intents;
        intents.extend(predictive_intents);
        if intents.is_empty() {
            if let Some(intent) = heuristic_idle.or(predictive_idle) {
                intents.push(intent);
            }
        }

        let mut rationale_tags = heuristic.rationale_tags;
        merge_tags(&mut rationale_tags, predictive.rationale_tags);
        let error = merge_errors(heuristic.error, predictive.error);

        DecisionBackendResult {
            route: DecisionRoute::LocalEvaluator,
            intent_batch: IntentBatch {
                window_id: input.current_context.window_id.clone(),
                intents,
                generated_at_ms: input.current_context.window_end_ms,
                model: "local-evaluator+predictive-local-v0.1".into(),
            },
            rationale_tags,
            latency_us: heuristic.latency_us + predictive.latency_us,
            error,
        }
    }
}

impl DecisionBackend for CompositeLocalEvaluatorBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        let input = ModelInput::current_only(context.clone());
        self.evaluate_model_input(&input)
    }

    fn evaluate_model_input(&self, input: &ModelInput) -> DecisionBackendResult {
        let heuristic = self.heuristic.evaluate_model_input(input);
        let predictive = self.predictive.evaluate_model_input(input);
        self.merge_results(input, heuristic, predictive)
    }
}

fn take_idle_noop(intents: &mut Vec<Intent>) -> Option<Intent> {
    intents
        .iter()
        .position(is_idle_noop)
        .map(|idx| intents.remove(idx))
}

fn is_idle_noop(intent: &Intent) -> bool {
    matches!(intent.intent_type, IntentType::Idle)
        && intent
            .suggested_actions
            .iter()
            .all(|action| action.action_type == ActionType::NoOp)
}

fn merge_tags(target: &mut Vec<String>, source: Vec<String>) {
    for tag in source {
        if !target.contains(&tag) {
            target.push(tag);
        }
    }
}

fn merge_errors(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(format!("{left}; {right}")),
        (Some(error), None) | (None, Some(error)) => Some(error),
        (None, None) => None,
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
    /// Optional runtime user id supplied by the environment. This is a stop-gap
    /// until the daemon/collector plumbs a real `UserBehaviorProfile` through
    /// `ModelInput`. When present it is attached to the default behavior profile
    /// so personalized models (e.g. `PredictiveLocalBackend`) can key per-user
    /// transition tables.
    runtime_user_id: Option<String>,
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

        let local_evaluator: Box<dyn DecisionBackend + Send + Sync> = match std::env::var(
            "DIPECS_NEXT_APP_MODEL_PATH",
        ) {
            Ok(path) if !path.trim().is_empty() => match PredictiveLocalBackend::from_path(&path) {
                Ok(backend) => Box::new(CompositeLocalEvaluatorBackend::new(backend)),
                Err(error) => {
                    tracing::warn!(error = %error, "next-app model ignored; falling back to heuristic local evaluator");
                    Box::new(LocalEvaluatorBackend)
                },
            },
            _ => Box::new(LocalEvaluatorBackend),
        };

        let runtime_user_id = std::env::var("DIPECS_NEXT_APP_USER_ID")
            .ok()
            .filter(|s| !s.trim().is_empty());

        Self {
            config,
            rule_based: Box::new(RuleBasedBackend),
            local_evaluator,
            fallback: Box::new(FallbackNoOpBackend),
            cloud_llm,
            cloud_disabled,
            cloud_misconfigured,
            circuit_state: RefCell::new(CircuitState::default()),
            runtime_user_id,
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
        Self::with_backends_and_runtime_user_id(
            config,
            rule_based,
            local_evaluator,
            cloud_llm,
            fallback,
            None,
        )
    }

    #[cfg(test)]
    fn with_backends_and_runtime_user_id(
        config: RouterConfig,
        rule_based: Box<dyn DecisionBackend + Send + Sync>,
        local_evaluator: Box<dyn DecisionBackend + Send + Sync>,
        cloud_llm: Option<Box<dyn DecisionBackend + Send + Sync>>,
        fallback: Box<dyn DecisionBackend + Send + Sync>,
        runtime_user_id: Option<String>,
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
            runtime_user_id,
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

    /// Evaluate with an explicit behavior profile. Callers that already have a
    /// `UserBehaviorProfile` (e.g. the daemon after aggregating recent feedback)
    /// should use this so personalized backends receive `user_id` and history.
    pub fn evaluate_with_profile(
        &self,
        context: &StructuredContext,
        profile: aios_spec::UserBehaviorProfile,
    ) -> DecisionBackendResult {
        let mut input = ModelInput::current_only(context.clone());
        input.behavior_profile = profile;
        self.evaluate_model_input(&input)
    }

    /// Evaluate a model input that includes the current window plus optional
    /// behavior profile and recent feedback memory.
    pub fn evaluate_model_input(&self, input: &ModelInput) -> DecisionBackendResult {
        let owned_input;
        let input = if input.behavior_profile.user_id.is_none() {
            if let Some(user_id) = &self.runtime_user_id {
                owned_input = {
                    let mut input = input.clone();
                    input.behavior_profile.user_id = Some(user_id.clone());
                    input
                };
                &owned_input
            } else {
                input
            }
        } else {
            input
        };
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

        // Priority 2: Privacy sensitivity.
        //
        // App transitions are deliberately *not* counted as privacy-sensitive
        // (they carry no raw text), so this gate only fires for notifications
        // with VerificationCode / FinancialContext hints.
        let privacy_score = Self::compute_privacy_score(context);
        if privacy_score > self.config.privacy_score_threshold {
            return (
                DecisionRoute::RuleBased,
                RoutingReason::PrivacySensitive {
                    score: privacy_score,
                },
            );
        }

        // Priority 3: Low-risk proactive local signals.
        //
        // LocalEvaluator owns prefetch, process prewarm, and work-scoped
        // keepalive. These are exactly the low-risk signals the next-app
        // predictor needs (foreground transitions, file activity, and
        // non-sensitive actionable notifications). Since the privacy gate above
        // no longer counts AppTransition, app-transition-heavy windows are not
        // trapped in RuleBased, while truly sensitive notification windows
        // still route conservatively.
        if Self::has_local_actionable_signal(context) {
            return (
                DecisionRoute::LocalEvaluator,
                RoutingReason::LocalActionableSignal,
            );
        }

        // Priority 4: Semantic complexity
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

    /// Count privacy-sensitive signals. Only notification events carrying
    /// `VerificationCode` or `FinancialContext` hints are counted; app
    /// transitions carry no raw text and are excluded from this score, so they
    /// pass through the privacy gate unchanged and are then handled by
    /// `has_local_actionable_signal` below.
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
mod tests;
