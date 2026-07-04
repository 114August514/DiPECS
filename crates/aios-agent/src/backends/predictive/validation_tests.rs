use std::collections::BTreeMap;

use super::{
    AppScore, EnsembleCombiner, FeatureLiftModel, LogisticRerankerModel, MarkovModel,
    NaiveBayesModel, NextAppModelArtifact, NextAppModelConfig, NextAppPredictor, TrainingSummary,
};

fn base_artifact() -> NextAppModelArtifact {
    let app_vocab = vec!["com.chat".into(), "com.mail".into()];
    NextAppModelArtifact {
        schema_version: "dipecs.next_app_model.v1".into(),
        model_id: "unit".into(),
        dataset_id: "unit".into(),
        trained_at_ms: 0,
        config: NextAppModelConfig::default(),
        app_vocab: app_vocab.clone(),
        global_popularity: vec![
            AppScore {
                app: "com.chat".into(),
                score: 0.6,
            },
            AppScore {
                app: "com.mail".into(),
                score: 0.4,
            },
        ],
        naive_bayes: NaiveBayesModel {
            class_log_priors: vec![0.0; app_vocab.len()],
            unknown_feature_log_probs: vec![0.0; app_vocab.len()],
            feature_log_probs: BTreeMap::new(),
        },
        markov: MarkovModel {
            global_transitions: BTreeMap::new(),
            user_transitions: BTreeMap::new(),
            global_transitions_order2: BTreeMap::new(),
        },
        feature_lift: FeatureLiftModel {
            base_scores: vec![0.0; app_vocab.len()],
            trees: Vec::new(),
        },
        user_frequency: BTreeMap::new(),
        user_recency: BTreeMap::new(),
        markov_context: BTreeMap::new(),
        ensemble_combiner: EnsembleCombiner::default(),
        ensemble_logistic: LogisticRerankerModel::default(),
        training_summary: TrainingSummary {
            examples: 1,
            users: 1,
            apps: app_vocab.len(),
        },
    }
}

#[test]
fn artifact_validation_rejects_unknown_user_frequency_app() {
    let mut artifact = base_artifact();
    artifact.user_frequency.insert(
        "u1".into(),
        vec![AppScore {
            app: "com.unknown".into(),
            score: 1.0,
        }],
    );

    assert!(
        NextAppPredictor::new(artifact).is_err(),
        "user_frequency scores outside app_vocab must be rejected"
    );
}

#[test]
fn artifact_validation_rejects_unknown_user_recency_target() {
    let mut artifact = base_artifact();
    artifact
        .user_recency
        .insert("u1\tcom.chat".into(), "com.unknown".into());

    assert!(
        NextAppPredictor::new(artifact).is_err(),
        "user_recency target outside app_vocab must be rejected"
    );
}

#[test]
fn artifact_validation_rejects_unknown_user_recency_current_app() {
    let mut artifact = base_artifact();
    artifact
        .user_recency
        .insert("u1\tcom.unknown".into(), "com.mail".into());

    assert!(
        NextAppPredictor::new(artifact).is_err(),
        "user_recency current-app keys outside app_vocab must be rejected"
    );
}

#[test]
fn artifact_validation_rejects_malformed_combiner_vectors() {
    let mut artifact = base_artifact();
    artifact.ensemble_combiner = EnsembleCombiner {
        components: vec!["markov".into()],
        weights: Vec::new(),
    };

    assert!(
        NextAppPredictor::new(artifact).is_err(),
        "partial ensemble_combiner vectors must be rejected"
    );
}

#[test]
fn artifact_validation_rejects_unknown_combiner_component() {
    let mut artifact = base_artifact();
    artifact.ensemble_combiner = EnsembleCombiner {
        components: vec!["unknown_component".into()],
        weights: vec![1.0],
    };

    assert!(
        NextAppPredictor::new(artifact).is_err(),
        "unknown ensemble_combiner components must be rejected"
    );
}
