use super::{
    backend::features_from_model_input, train_next_app_artifact, AppScore, FeatureLiftModel,
    MarkovModel, NaiveBayesModel, NextAppAlgorithm, NextAppModelArtifact, NextAppModelConfig,
    NextAppPredictor, NextAppTrainingExample, PredictionFeatures, PredictiveLocalBackend,
    TrainingSummary,
};
use crate::DecisionBackend;
use aios_spec::{
    ActionType, ContextSummary, DecisionRoute, IntentType, ModelInput, RiskLevel, SanitizedEvent,
    SourceTier, StructuredContext, SystemStatusSnapshot,
};

fn examples() -> Vec<NextAppTrainingExample> {
    vec![
        example("u1", "com.chat", &[], "com.mail"),
        example("u1", "com.chat", &["com.home"], "com.mail"),
        example("u2", "com.chat", &[], "com.mail"),
        example("u2", "com.mail", &["com.chat"], "com.browser"),
        example("u3", "com.chat", &[], "com.browser"),
    ]
}

fn example(
    user_id: &str,
    current_app: &str,
    history: &[&str],
    label_app: &str,
) -> NextAppTrainingExample {
    NextAppTrainingExample {
        user_id: user_id.into(),
        current_app: current_app.into(),
        history: history.iter().map(|app| (*app).into()).collect(),
        hour_bucket: 9,
        weekday: 1,
        event_type: "app_usage".into(),
        label_app: label_app.into(),
    }
}

#[test]
fn markov_ranks_observed_transition_first() {
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
        .expect("training should succeed");
    let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");
    let features = PredictionFeatures {
        current_app: Some("com.chat".into()),
        ..PredictionFeatures::default()
    };

    let ranked = predictor.rank(&features, NextAppAlgorithm::Markov, 3);

    assert_eq!(ranked[0].app, "com.mail");
    assert!(ranked[0].score > ranked[1].score);
}

#[test]
fn malformed_artifact_is_rejected() {
    let mut artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
        .expect("training should succeed");
    artifact.naive_bayes.class_log_priors.pop();

    assert!(NextAppPredictor::new(artifact).is_err());
}

#[test]
fn backend_emits_policy_safe_action_for_unobserved_prediction() {
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
        .expect("training should succeed");
    let backend = PredictiveLocalBackend::new(artifact).expect("backend should construct");
    let ctx = context_with_foreground("com.chat");

    let result = backend.evaluate(&ctx);
    let first = &result.intent_batch.intents[0];

    assert_eq!(result.route, DecisionRoute::LocalEvaluator);
    assert!(matches!(first.intent_type, IntentType::OpenApp(_)));
    assert_eq!(first.risk_level, RiskLevel::Medium);
    assert_eq!(
        first.suggested_actions[0].action_type,
        ActionType::KeepAlive
    );
    assert_eq!(
        first.suggested_actions[0].target.as_deref(),
        Some("work:collector_heartbeat")
    );
}

#[test]
fn backend_uses_behavior_profile_user_id_for_personalized_markov() {
    // u1: chat -> mail every time; u2: chat -> browser every time.
    let train = vec![
        example("u1", "com.chat", &[], "com.mail"),
        example("u1", "com.chat", &[], "com.mail"),
        example("u1", "com.chat", &[], "com.mail"),
        example("u2", "com.chat", &[], "com.browser"),
        example("u2", "com.chat", &[], "com.browser"),
        example("u2", "com.chat", &[], "com.browser"),
    ];
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &train)
        .expect("training should succeed");
    let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");
    let ctx = context_with_foreground("com.chat");

    let mut input = ModelInput::current_only(ctx.clone());
    input.behavior_profile.user_id = Some("u1".into());
    let features = features_from_model_input(&input);
    let ranked = predictor.rank(&features, NextAppAlgorithm::Markov, 3);
    assert_eq!(
        ranked[0].app, "com.mail",
        "with user_id=u1 Markov should rank com.mail first"
    );

    let mut input = ModelInput::current_only(ctx);
    input.behavior_profile.user_id = Some("u2".into());
    let features = features_from_model_input(&input);
    let ranked = predictor.rank(&features, NextAppAlgorithm::Markov, 3);
    assert_eq!(
        ranked[0].app, "com.browser",
        "with user_id=u2 Markov should rank com.browser first"
    );
}

#[test]
fn ensemble_considers_candidates_beyond_each_component_top_10() {
    let apps: Vec<String> = (0..12).map(|idx| format!("com.app{idx:02}")).collect();
    let component_scores: Vec<AppScore> = apps
        .iter()
        .enumerate()
        .map(|(idx, app)| AppScore {
            app: app.clone(),
            score: 1.0 - idx as f32 * 0.01,
        })
        .collect();
    let artifact = NextAppModelArtifact {
        schema_version: "dipecs.next_app_model.v1".into(),
        model_id: "unit".into(),
        dataset_id: "unit".into(),
        trained_at_ms: 0,
        config: NextAppModelConfig::default(),
        app_vocab: apps.clone(),
        global_popularity: component_scores.clone(),
        naive_bayes: NaiveBayesModel {
            class_log_priors: vec![0.0; apps.len()],
            unknown_feature_log_probs: vec![0.0; apps.len()],
            feature_log_probs: std::collections::BTreeMap::new(),
        },
        markov: MarkovModel {
            global_transitions: std::collections::BTreeMap::from([(
                "com.current".into(),
                component_scores,
            )]),
            user_transitions: std::collections::BTreeMap::new(),
        },
        feature_lift: FeatureLiftModel {
            base_scores: vec![0.0; apps.len()],
            trees: vec![],
        },
        training_summary: TrainingSummary {
            examples: 1,
            users: 1,
            apps: apps.len(),
        },
    };
    let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");
    let features = PredictionFeatures {
        current_app: Some("com.current".into()),
        ..PredictionFeatures::default()
    };

    let ranked = predictor.rank(&features, NextAppAlgorithm::Ensemble, apps.len());
    let ranked_apps: Vec<&str> = ranked.iter().map(|score| score.app.as_str()).collect();

    assert!(
        ranked_apps.contains(&"com.app11"),
        "ensemble must preserve long-tail candidates from full component rankings"
    );
}

#[test]
fn backend_emits_prewarm_for_in_context_prediction() {
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
        .expect("training should succeed");
    let backend = PredictiveLocalBackend::new(artifact).expect("backend should construct");
    let mut ctx = context_with_foreground("com.chat");
    // Make com.mail observable in the current context so the prediction is
    // considered in-context and safe to prewarm.
    ctx.summary.notified_apps.push("com.mail".into());

    let result = backend.evaluate(&ctx);
    let first = &result.intent_batch.intents[0];

    assert!(matches!(
        &first.intent_type,
        IntentType::SwitchToApp(app) if app == "com.mail"
    ));
    assert_eq!(first.risk_level, RiskLevel::Low);
    assert_eq!(
        first.suggested_actions[0].action_type,
        ActionType::PreWarmProcess
    );
    assert_eq!(
        first.suggested_actions[0].target.as_deref(),
        Some("pkg:com.mail")
    );
}

fn context_with_foreground(package: &str) -> StructuredContext {
    StructuredContext {
        window_id: "w1".into(),
        window_start_ms: 0,
        window_end_ms: 1_000,
        duration_secs: 1,
        events: vec![SanitizedEvent {
            event_id: "e1".into(),
            timestamp_ms: 1_000,
            event_type: aios_spec::SanitizedEventType::AppTransition {
                package_name: package.into(),
                activity_class: None,
                transition: aios_spec::AppTransition::Foreground,
            },
            source_tier: SourceTier::PublicApi,
            app_package: Some(package.into()),
            uid: None,
        }],
        summary: ContextSummary {
            foreground_apps: vec![package.into()],
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: Option::<SystemStatusSnapshot>::None,
            source_tier: SourceTier::PublicApi,
        },
    }
}
