use super::{
    backend::features_from_model_input, train_next_app_artifact, AppScore, FeatureLiftModel,
    MarkovModel, NaiveBayesModel, NextAppAlgorithm, NextAppModelArtifact, NextAppModelConfig,
    NextAppPredictor, NextAppTrainingExample, PredictionFeatures, PredictiveLocalBackend,
    TrainingSummary,
};
use crate::DecisionBackend;
use aios_core::policy_engine::PolicyEngine;
use aios_spec::PolicyVerdict;
use aios_spec::{
    ActionType, CapabilityLevel, ContextSummary, DecisionRoute, IntentType, ModelInput, RiskLevel,
    SanitizedEvent, SourceTier, StructuredContext, SystemStatusSnapshot,
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
fn training_builds_order2_markov_table() {
    // Two examples share the (prev=com.home, current=com.chat) context but lead
    // to different next apps; order-2 must record the (prev,current) key.
    let examples = vec![
        example("u1", "com.chat", &["com.home"], "com.mail"),
        example("u1", "com.chat", &["com.home"], "com.mail"),
        example("u2", "com.chat", &["com.music"], "com.browser"),
    ];
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples)
        .expect("training should succeed");

    // The order-2 table is keyed "prev\tcurrent".
    let key = "com.home\tcom.chat";
    let scores = artifact
        .markov
        .global_transitions_order2
        .get(key)
        .expect("order-2 key should exist");
    assert_eq!(scores[0].app, "com.mail");
}

#[test]
fn order2_runtime_skips_repeated_current_app_history_entry() {
    // Training derives the order-2 key from the latest history app that is not
    // the current app. Runtime lookup must use the same rule, otherwise a
    // repeated foreground record at the end of history misses the learned key.
    let examples = vec![
        example("u1", "com.chat", &["com.home", "com.chat"], "com.mail"),
        example("u2", "com.chat", &["com.home", "com.chat"], "com.mail"),
    ];
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples)
        .expect("training should succeed");
    let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");
    let features = PredictionFeatures {
        current_app: Some("com.chat".into()),
        history: vec!["com.home".into(), "com.chat".into()],
        ..PredictionFeatures::default()
    };

    let rankings = predictor.component_rankings(&features);
    let (_, order2) = rankings
        .iter()
        .find(|(name, _)| *name == "markov_order2")
        .expect("order-2 component should exist");

    assert_eq!(
        order2.first().map(|score| score.app.as_str()),
        Some("com.mail"),
        "order-2 runtime lookup should match training's previous-app rule"
    );
}

#[test]
fn trained_artifact_has_nonempty_learned_combiner() {
    // With enough examples to carve a validation slice, the trainer should fit
    // and lock a non-empty combiner over the documented component set.
    let mut examples = Vec::new();
    for i in 0..60 {
        let user = format!("u{}", i % 5);
        examples.push(example(&user, "com.chat", &["com.home"], "com.mail"));
        examples.push(example(&user, "com.mail", &["com.chat"], "com.browser"));
    }
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples)
        .expect("training should succeed");

    assert!(
        !artifact.ensemble_combiner.is_empty(),
        "combiner should be fit when validation slice is non-empty"
    );
    assert_eq!(
        artifact.ensemble_combiner.components.len(),
        artifact.ensemble_combiner.weights.len(),
        "components and weights must stay parallel"
    );
    // The combiner must be usable by the ensemble path without panicking.
    let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");
    let features = PredictionFeatures {
        user_id: Some("u0".into()),
        current_app: Some("com.chat".into()),
        history: vec!["com.home".into()],
        ..PredictionFeatures::default()
    };
    let ranked = predictor.rank(&features, NextAppAlgorithm::Ensemble, 3);
    assert!(!ranked.is_empty(), "ensemble should produce predictions");
}

#[test]
fn trained_artifact_has_logistic_reranker_for_hard_user_recency() {
    // The strong baseline's advantage comes from a hard "last transition"
    // pointer. The ensemble should learn that same signal as a candidate
    // feature instead of only treating per-user Markov as a diffuse count table.
    let mut examples = Vec::new();
    for i in 0..80 {
        let user = format!("u{}", i % 8);
        examples.push(example(&user, "com.chat", &["com.home"], "com.mail"));
        examples.push(example(&user, "com.chat", &["com.home"], "com.browser"));
        examples.push(example(&user, "com.chat", &["com.home"], "com.calendar"));
        examples.push(example(&user, "com.mail", &["com.chat"], "com.docs"));
    }
    examples.push(example("target", "com.chat", &["com.home"], "com.browser"));
    examples.push(example("target", "com.chat", &["com.home"], "com.calendar"));
    examples.push(example("target", "com.chat", &["com.home"], "com.mail"));

    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples)
        .expect("training should succeed");
    assert!(
        !artifact.ensemble_logistic.is_empty(),
        "trainer should fit a non-empty logistic reranker when validation data exists"
    );

    let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");
    let features = PredictionFeatures {
        user_id: Some("target".into()),
        current_app: Some("com.chat".into()),
        history: vec!["com.home".into()],
        ..PredictionFeatures::default()
    };

    let ranked = predictor.rank(&features, NextAppAlgorithm::Ensemble, 3);

    assert_eq!(
        ranked[0].app, "com.mail",
        "logistic reranker should learn to trust the hard last-transition pointer"
    );
}

#[test]
fn trained_artifact_uses_deterministic_timestamp() {
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
        .expect("training should succeed");

    assert_eq!(
        artifact.trained_at_ms, 0,
        "generated artifacts must be byte-stable across equivalent training runs"
    );
}

#[test]
fn artifact_validation_rejects_duplicate_or_unknown_app_scores() {
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
        .expect("training should succeed");

    let mut duplicate_vocab = artifact.clone();
    duplicate_vocab.app_vocab[1] = duplicate_vocab.app_vocab[0].clone();
    assert!(
        NextAppPredictor::new(duplicate_vocab).is_err(),
        "duplicate app_vocab entries must be rejected"
    );

    let mut unknown_global = artifact.clone();
    unknown_global.global_popularity[0].app = "com.unknown".into();
    assert!(
        NextAppPredictor::new(unknown_global).is_err(),
        "global_popularity entries outside app_vocab must be rejected"
    );

    let mut duplicate_markov = artifact.clone();
    let scores = duplicate_markov
        .markov
        .global_transitions
        .get_mut("com.chat")
        .expect("training fixture should have chat transitions");
    if scores.len() >= 2 {
        scores[1].app = scores[0].app.clone();
    }
    assert!(
        NextAppPredictor::new(duplicate_markov).is_err(),
        "duplicate transition score apps must be rejected"
    );

    let mut malformed_logistic = artifact.clone();
    malformed_logistic.ensemble_logistic = super::LogisticRerankerModel {
        feature_names: vec!["unknown:rank_rr".into()],
        weights: vec![],
    };
    assert!(
        NextAppPredictor::new(malformed_logistic).is_err(),
        "malformed logistic reranker weights must be rejected"
    );

    let mut unknown_tree = artifact;
    unknown_tree.feature_lift.trees[0].yes_scores[0].app = "com.unknown".into();
    assert!(
        NextAppPredictor::new(unknown_tree).is_err(),
        "feature-lift tree scores outside app_vocab must be rejected"
    );
}

#[test]
fn fallback_does_not_reintroduce_current_app_after_filtering() {
    let artifact = NextAppModelArtifact {
        schema_version: "dipecs.next_app_model.v1".into(),
        model_id: "unit".into(),
        dataset_id: "unit".into(),
        trained_at_ms: 0,
        config: NextAppModelConfig::default(),
        app_vocab: vec!["com.current".into()],
        global_popularity: vec![AppScore {
            app: "com.current".into(),
            score: 1.0,
        }],
        naive_bayes: NaiveBayesModel {
            class_log_priors: vec![0.0],
            unknown_feature_log_probs: vec![0.0],
            feature_log_probs: std::collections::BTreeMap::new(),
        },
        markov: MarkovModel {
            global_transitions: std::collections::BTreeMap::new(),
            user_transitions: std::collections::BTreeMap::new(),
            global_transitions_order2: std::collections::BTreeMap::new(),
        },
        feature_lift: FeatureLiftModel {
            base_scores: vec![0.0],
            trees: vec![],
        },
        user_frequency: std::collections::BTreeMap::new(),
        user_recency: std::collections::BTreeMap::new(),
        markov_context: std::collections::BTreeMap::new(),
        ensemble_combiner: super::EnsembleCombiner::default(),
        ensemble_logistic: super::LogisticRerankerModel::default(),
        training_summary: TrainingSummary {
            examples: 1,
            users: 1,
            apps: 1,
        },
    };
    let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");
    let features = PredictionFeatures {
        current_app: Some("com.current".into()),
        ..PredictionFeatures::default()
    };

    let ranked = predictor.rank(&features, NextAppAlgorithm::Markov, 1);

    assert!(
        ranked.is_empty(),
        "fallback global popularity must still respect current-app filtering"
    );
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
    assert_eq!(first.risk_level, RiskLevel::Low);
    assert_eq!(
        first.suggested_actions[0].action_type,
        ActionType::KeepAlive
    );
    assert_eq!(
        first.suggested_actions[0].target.as_deref(),
        Some("work:collector_heartbeat")
    );

    let decisions = PolicyEngine::default().evaluate_batch_with_context(
        &result.intent_batch,
        &CapabilityLevel::for_route(result.route),
        &ctx,
    );
    assert!(
        decisions
            .iter()
            .all(|decision| matches!(decision.verdict, PolicyVerdict::Approved)),
        "work-scoped keepalive fallback must be approved by LocalEvaluator policy: {decisions:?}"
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
    let mut app_vocab = apps.clone();
    app_vocab.push("com.current".into());
    let component_scores: Vec<AppScore> = apps
        .iter()
        .enumerate()
        .map(|(idx, app)| AppScore {
            app: app.clone(),
            score: 1.0 - idx as f32 * 0.01,
        })
        .collect();
    let mut global_popularity = component_scores.clone();
    global_popularity.push(AppScore {
        app: "com.current".into(),
        score: 0.0,
    });
    let artifact = NextAppModelArtifact {
        schema_version: "dipecs.next_app_model.v1".into(),
        model_id: "unit".into(),
        dataset_id: "unit".into(),
        trained_at_ms: 0,
        config: NextAppModelConfig::default(),
        app_vocab: app_vocab.clone(),
        global_popularity,
        naive_bayes: NaiveBayesModel {
            class_log_priors: vec![0.0; app_vocab.len()],
            unknown_feature_log_probs: vec![0.0; app_vocab.len()],
            feature_log_probs: std::collections::BTreeMap::new(),
        },
        markov: MarkovModel {
            global_transitions: std::collections::BTreeMap::from([(
                "com.current".into(),
                component_scores,
            )]),
            user_transitions: std::collections::BTreeMap::new(),
            global_transitions_order2: std::collections::BTreeMap::new(),
        },
        feature_lift: FeatureLiftModel {
            base_scores: vec![0.0; app_vocab.len()],
            trees: vec![],
        },
        user_frequency: std::collections::BTreeMap::new(),
        user_recency: std::collections::BTreeMap::new(),
        markov_context: std::collections::BTreeMap::new(),
        ensemble_combiner: super::EnsembleCombiner::default(),
        ensemble_logistic: super::LogisticRerankerModel::default(),
        training_summary: TrainingSummary {
            examples: 1,
            users: 1,
            apps: app_vocab.len(),
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

#[test]
fn markov_context_ranker_prefers_hour_specific_app() {
    // u1: chat -> mail at hour 8, chat -> browser at hour 21
    let mut train = Vec::new();
    for _ in 0..10 {
        train.push(NextAppTrainingExample {
            user_id: "u1".into(),
            current_app: "com.chat".into(),
            history: vec![],
            hour_bucket: 8,
            weekday: 1,
            event_type: "foreground".into(),
            label_app: "com.mail".into(),
        });
        train.push(NextAppTrainingExample {
            user_id: "u1".into(),
            current_app: "com.chat".into(),
            history: vec![],
            hour_bucket: 21,
            weekday: 1,
            event_type: "foreground".into(),
            label_app: "com.browser".into(),
        });
    }
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &train)
        .expect("training should succeed");
    let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");

    let ranked = predictor.rank(
        &PredictionFeatures {
            current_app: Some("com.chat".into()),
            hour_bucket: Some(8),
            ..PredictionFeatures::default()
        },
        NextAppAlgorithm::Ensemble,
        3,
    );
    assert!(
        ranked.iter().any(|s| s.app == "com.mail"),
        "hour=8 should boost com.mail: {ranked:?}"
    );
}

#[test]
fn markov_context_ranker_falls_back_to_global_when_no_temporal_data() {
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
        .expect("training should succeed");
    let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");
    let features = PredictionFeatures {
        current_app: Some("com.chat".into()),
        ..PredictionFeatures::default()
    };

    // markov_context returns empty when no hour/weekday provided
    let rankings = predictor.component_rankings(&features);
    let (_, ctx_scores) = rankings
        .iter()
        .find(|(name, _)| *name == "markov_context")
        .expect("markov_context component should exist");
    assert!(
        ctx_scores.is_empty(),
        "markov_context should return empty without temporal features"
    );
}

#[test]
fn markov_context_component_appears_in_ensemble_components() {
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &examples())
        .expect("training should succeed");
    let predictor = NextAppPredictor::new(artifact).expect("artifact should validate");
    let features = PredictionFeatures {
        current_app: Some("com.chat".into()),
        ..PredictionFeatures::default()
    };

    let names: Vec<&str> = predictor
        .component_rankings(&features)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(
        names.contains(&"markov_context"),
        "markov_context must be registered in ensemble components: {names:?}"
    );
}

#[test]
fn trained_artifact_has_markov_context_transitions() {
    let mut train = Vec::new();
    for i in 0..10 {
        train.push(NextAppTrainingExample {
            user_id: format!("u{}", i % 3),
            current_app: "com.chat".into(),
            history: vec![],
            hour_bucket: 8,
            weekday: 1,
            event_type: "foreground".into(),
            label_app: "com.mail".into(),
        });
    }
    let artifact = train_next_app_artifact("unit", NextAppModelConfig::default(), &train)
        .expect("training should succeed");

    let hour_key = "com.chat\t8";
    assert!(
        artifact.markov_context.contains_key(hour_key),
        "markov_context should contain hour-keyed transitions for {hour_key}"
    );
    let weekday_key = "com.chat\t1";
    assert!(
        artifact.markov_context.contains_key(weekday_key),
        "markov_context should contain weekday-keyed transitions for {weekday_key}"
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
