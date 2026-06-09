//! DecisionRouter — 多层级决策路由。
//!
//! 路由优先级：
//! 1. Circuit breaker — 连续错误超阈值 → FallbackNoOp
//! 2. Privacy sensitivity — 敏感信号过多 → RuleBased 降级
//! 3. Semantic complexity — 信号种类数决定后端（当前统一收敛到 RuleBased）

use std::cell::RefCell;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use aios_spec::{
    DecisionBackendResult, DecisionRoute, SanitizedEventType, SemanticHint, StructuredContext,
};

use crate::backends::cloud_llm::CloudBackendState;
use crate::backends::fallback::FallbackNoOpBackend;
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
    LowComplexity,
    CloudPreferred { complexity: &'static str },
    CloudDisabled { complexity: &'static str },
    CloudMisconfigured { complexity: &'static str },
}

impl RoutingReason {
    fn tag(&self) -> String {
        match self {
            RoutingReason::CircuitBreakerTripped { failure_count } => {
                format!("routing:circuit_breaker_fallback(errors={})", failure_count)
            },
            RoutingReason::PrivacySensitive { score } => {
                format!("routing:privacy_sensitive(score={})", score)
            },
            RoutingReason::LowComplexity => "routing:low_complexity".into(),
            RoutingReason::CloudPreferred { complexity } => {
                format!("routing:{complexity}_complexity(cloud_llm)")
            },
            RoutingReason::CloudDisabled { complexity } => {
                format!("routing:{complexity}_complexity(rule_based_cloud_disabled)")
            },
            RoutingReason::CloudMisconfigured { complexity } => {
                format!("routing:{complexity}_complexity(rule_based_cloud_misconfigured)")
            },
        }
    }
}

// ============================================================
// DecisionRouter
// ============================================================

pub struct DecisionRouter {
    config: RouterConfig,
    rule_based: RuleBasedBackend,
    fallback: FallbackNoOpBackend,
    cloud_llm: CloudBackendState,
    circuit_state: RefCell<CircuitState>,
}

impl DecisionRouter {
    pub fn new(config: RouterConfig) -> Self {
        let cloud_llm = CloudBackendState::from_env();
        if let CloudBackendState::Misconfigured(error) = &cloud_llm {
            tracing::warn!(
                error = %error,
                "cloud llm backend configuration ignored; DecisionRouter will stay local"
            );
        }
        Self {
            config,
            rule_based: RuleBasedBackend,
            fallback: FallbackNoOpBackend,
            cloud_llm,
            circuit_state: RefCell::new(CircuitState::default()),
        }
    }

    /// Evaluate a StructuredContext through the routing pipeline.
    ///
    /// Uses interior mutability (`RefCell`) to track circuit breaker state
    /// across calls without requiring `&mut self`.
    pub fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        let (route, reason) = self.determine_route(context);
        let reason_tag = reason.tag();

        let mut result = match route {
            DecisionRoute::RuleBased => self.rule_based.evaluate(context),
            DecisionRoute::CloudLlm => match &self.cloud_llm {
                CloudBackendState::Ready(backend) => {
                    let cloud_result = backend.evaluate(context);
                    if let Some(error) = cloud_result.error.clone() {
                        let mut fallback = self.rule_based.evaluate(context);
                        fallback.error = Some(format!("cloud llm backend failed: {error}"));
                        fallback
                            .rationale_tags
                            .push("backend:cloud_llm_error(rule_based_fallback)".to_string());
                        fallback
                    } else {
                        cloud_result
                    }
                },
                _ => self.rule_based.evaluate(context),
            },
            DecisionRoute::FallbackNoOp => self.fallback.evaluate(context),
            // Future routes (LocalEvaluator) fall back to RuleBased
            _ => self.rule_based.evaluate(context),
        };

        // Inject routing reason tag
        if !result.rationale_tags.iter().any(|tag| tag == &reason_tag) {
            result.rationale_tags.push(reason_tag);
        }

        // Update circuit breaker state
        let mut state = self.circuit_state.borrow_mut();
        if result.error.is_some() {
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

        // Priority 3: Semantic complexity
        let unique_types = Self::count_unique_semantic_hint_types(context);
        match unique_types {
            0 | 1 => (DecisionRoute::RuleBased, RoutingReason::LowComplexity),
            2 | 3 => self.cloud_route_or_fallback("medium"),
            _ => self.cloud_route_or_fallback("high"),
        }
    }

    fn cloud_route_or_fallback(&self, complexity: &'static str) -> (DecisionRoute, RoutingReason) {
        match &self.cloud_llm {
            CloudBackendState::Ready(_) => (
                DecisionRoute::CloudLlm,
                RoutingReason::CloudPreferred { complexity },
            ),
            CloudBackendState::Disabled => (
                DecisionRoute::RuleBased,
                RoutingReason::CloudDisabled { complexity },
            ),
            CloudBackendState::Misconfigured(_) => (
                DecisionRoute::RuleBased,
                RoutingReason::CloudMisconfigured { complexity },
            ),
        }
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
