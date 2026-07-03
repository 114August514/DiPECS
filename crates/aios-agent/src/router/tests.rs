use super::*;
use aios_spec::{
    ActionType, ActionUrgency, AppTransition, ContextSummary, DecisionBackendResult, DecisionRoute,
    Intent, IntentBatch, IntentType, RiskLevel, SanitizedEvent, SanitizedEventType, SemanticHint,
    SourceTier, StructuredContext, SuggestedAction,
};

use crate::DecisionBackend;

static ENV_LOCK: Mutex<()> = Mutex::new(());

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

struct EnvVarGuard {
    key: &'static str,
    old: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let old = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, old }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(old) = &self.old {
            std::env::set_var(self.key, old);
        } else {
            std::env::remove_var(self.key);
        }
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

struct UserIdCapturingBackend {
    seen_user_ids: Arc<Mutex<Vec<Option<String>>>>,
}

impl DecisionBackend for UserIdCapturingBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        DecisionBackendResult {
            route: DecisionRoute::LocalEvaluator,
            intent_batch: idle_batch(context),
            rationale_tags: vec!["capturing_backend".into()],
            latency_us: 0,
            error: None,
        }
    }

    fn evaluate_model_input(&self, input: &aios_spec::ModelInput) -> DecisionBackendResult {
        self.seen_user_ids
            .lock()
            .expect("capture mutex should not be poisoned")
            .push(input.behavior_profile.user_id.clone());
        self.evaluate(&input.current_context)
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

#[test]
fn app_transitions_route_to_local_evaluator_before_privacy_gate() {
    // Four foreground transitions would previously have scored 4 on the old
    // privacy metric and been trapped in RuleBased. They are now treated as
    // a local actionable signal and routed to LocalEvaluator so the
    // next-app predictor can see them.
    let ctx = StructuredContext {
        window_id: "transition-heavy-window".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events: vec![
            foreground_transition(100, "com.chat"),
            foreground_transition(200, "com.mail"),
            foreground_transition(300, "com.browser"),
            foreground_transition(400, "com.music"),
        ],
        summary: ContextSummary {
            foreground_apps: vec!["com.music".into()],
            notified_apps: vec![],
            all_semantic_hints: vec![],
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
    assert!(
        matches!(result.route, DecisionRoute::LocalEvaluator),
        "transition-heavy windows must reach LocalEvaluator, got {:?}",
        result.route
    );
    assert!(result
        .rationale_tags
        .iter()
        .any(|tag| tag == "routing:local_actionable_signal"));
}

#[test]
fn evaluate_model_input_injects_runtime_user_id_when_profile_lacks_one() {
    let seen_user_ids = Arc::new(Mutex::new(Vec::new()));
    let router = DecisionRouter::with_backends_and_runtime_user_id(
        RouterConfig::default(),
        Box::new(OkBackend {
            route: DecisionRoute::RuleBased,
        }),
        Box::new(UserIdCapturingBackend {
            seen_user_ids: Arc::clone(&seen_user_ids),
        }),
        None,
        Box::new(FallbackNoOpBackend),
        Some("runtime-user".into()),
    );
    let ctx = StructuredContext {
        window_id: "daemon-model-input-path".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events: vec![foreground_transition(100, "com.chat")],
        summary: ContextSummary {
            foreground_apps: vec!["com.chat".into()],
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    };
    let input = ModelInput::current_only(ctx);
    assert_eq!(input.behavior_profile.user_id, None);

    let result = router.evaluate_model_input(&input);

    assert!(matches!(result.route, DecisionRoute::LocalEvaluator));
    assert_eq!(
        seen_user_ids
            .lock()
            .expect("capture mutex should not be poisoned")
            .as_slice(),
        &[Some("runtime-user".into())],
        "daemon-style evaluate_model_input path must attach runtime user id"
    );
}

#[test]
fn next_app_model_composes_with_heuristic_local_evaluator() {
    let _env_guard = ENV_LOCK
        .lock()
        .expect("environment mutex should not be poisoned");
    let artifact = crate::backends::predictive::train_next_app_artifact(
        "unit",
        crate::backends::predictive::NextAppModelConfig::default(),
        &[
            next_app_example("u1", "com.chat", "com.mail"),
            next_app_example("u1", "com.chat", "com.mail"),
            next_app_example("u2", "com.chat", "com.mail"),
        ],
    )
    .expect("training should succeed");
    let dir = std::env::temp_dir().join(format!(
        "dipecs-router-compose-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let artifact_path = dir.join("artifact.json");
    serde_json::to_writer_pretty(
        std::fs::File::create(&artifact_path).expect("create artifact"),
        &artifact,
    )
    .expect("write artifact");
    let _model_path = EnvVarGuard::set("DIPECS_NEXT_APP_MODEL_PATH", artifact_path.as_os_str());

    let router = DecisionRouter::new(RouterConfig::default());
    let ctx = StructuredContext {
        window_id: "foreground-with-predictive-model".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events: vec![foreground_transition(100, "com.chat")],
        summary: ContextSummary {
            foreground_apps: vec!["com.chat".into()],
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    };

    let result = router.evaluate(&ctx);

    assert!(
        result.intent_batch.intents.iter().any(|intent| matches!(
            &intent.intent_type,
            IntentType::SwitchToApp(app) if app == "com.chat"
        )),
        "enabling the next-app model must preserve LocalEvaluator foreground intents"
    );
    assert!(
        result.intent_batch.intents.iter().any(|intent| matches!(
            &intent.intent_type,
            IntentType::OpenApp(app) if app == "com.mail"
        )),
        "enabling the next-app model must add predictive intents"
    );
    assert!(
        result
            .intent_batch
            .intents
            .iter()
            .flat_map(|intent| intent.suggested_actions.iter())
            .any(|action| action.action_type == ActionType::PreWarmProcess
                && action.target.as_deref() == Some("pkg:com.chat")),
        "heuristic LocalEvaluator prewarm should not disappear behind the model env var"
    );

    let _ = std::fs::remove_dir_all(dir);
}

fn next_app_example(
    user_id: &str,
    current_app: &str,
    label_app: &str,
) -> crate::backends::predictive::NextAppTrainingExample {
    crate::backends::predictive::NextAppTrainingExample {
        user_id: user_id.into(),
        current_app: current_app.into(),
        history: vec![],
        hour_bucket: 9,
        weekday: 1,
        event_type: "foreground".into(),
        label_app: label_app.into(),
    }
}

fn foreground_transition(timestamp_ms: i64, package_name: &str) -> SanitizedEvent {
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
