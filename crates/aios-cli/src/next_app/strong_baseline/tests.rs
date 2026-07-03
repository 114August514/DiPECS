use super::*;

fn example(user_id: &str, current_app: &str, label_app: &str) -> NextAppTrainingExample {
    example_with_context(
        user_id,
        vec![current_app.to_string()],
        current_app,
        12,
        1,
        "foreground",
        label_app,
    )
}

fn example_with_context(
    user_id: &str,
    history: Vec<String>,
    current_app: &str,
    hour_bucket: u8,
    weekday: u8,
    event_type: &str,
    label_app: &str,
) -> NextAppTrainingExample {
    NextAppTrainingExample {
        user_id: user_id.to_string(),
        current_app: current_app.to_string(),
        history,
        hour_bucket,
        weekday,
        event_type: event_type.to_string(),
        label_app: label_app.to_string(),
    }
}

#[test]
fn strong_baseline_predicts_markov_transition() {
    let examples = vec![
        example("u1", "A", "B"),
        example("u1", "A", "B"),
        example("u1", "A", "C"),
        example("u1", "B", "A"),
    ];
    let baseline = StrongPredictiveActionBaseline::from_training(&examples);
    let pred = baseline.predict_for_example(&example("u1", "A", "B"), 2);
    assert_eq!(pred[0], "B");
}

#[test]
fn strong_baseline_falls_back_to_popularity_for_unknown_context() {
    let examples = vec![
        example("u1", "A", "B"),
        example("u1", "A", "B"),
        example("u1", "A", "C"),
    ];
    let baseline = StrongPredictiveActionBaseline::from_training(&examples);
    let pred = baseline.predict_for_example(&example("u1", "UNKNOWN", "B"), 2);
    assert_eq!(pred[0], "B");
    assert_eq!(pred[1], "C");
}

#[test]
fn strong_baseline_uses_order2_markov_when_previous_app_is_known() {
    let examples = vec![
        example_with_context("u1", vec!["X".into(), "A".into()], "A", 9, 1, "fg", "B"),
        example_with_context("u2", vec!["Y".into(), "A".into()], "A", 9, 1, "fg", "C"),
        example_with_context("u3", vec!["Y".into(), "A".into()], "A", 9, 1, "fg", "C"),
        example_with_context("u4", vec!["Z".into(), "A".into()], "A", 9, 1, "fg", "C"),
    ];
    let baseline = StrongPredictiveActionBaseline::from_training(&examples);
    let query = example_with_context(
        "new-user",
        vec!["X".into(), "A".into()],
        "A",
        9,
        1,
        "fg",
        "B",
    );

    let pred = baseline.predict_for_example(&query, 2);

    assert_eq!(
        pred[0], "B",
        "previous-app context X->A should beat the weaker A->C majority"
    );
}

#[test]
fn strong_baseline_uses_context_bayes_for_unknown_current_app() {
    let examples = vec![
        example_with_context(
            "u1",
            vec!["launcher".into()],
            "launcher",
            8,
            1,
            "work",
            "mail",
        ),
        example_with_context(
            "u2",
            vec!["launcher".into()],
            "launcher",
            8,
            1,
            "work",
            "mail",
        ),
        example_with_context(
            "u3",
            vec!["launcher".into()],
            "launcher",
            8,
            1,
            "work",
            "mail",
        ),
        example_with_context(
            "u4",
            vec!["launcher".into()],
            "launcher",
            21,
            6,
            "leisure",
            "browser",
        ),
        example_with_context(
            "u5",
            vec!["launcher".into()],
            "launcher",
            21,
            6,
            "leisure",
            "browser",
        ),
        example_with_context(
            "u6",
            vec!["launcher".into()],
            "launcher",
            21,
            6,
            "leisure",
            "browser",
        ),
        example_with_context(
            "u7",
            vec!["launcher".into()],
            "launcher",
            21,
            6,
            "leisure",
            "browser",
        ),
        example_with_context(
            "u8",
            vec!["launcher".into()],
            "launcher",
            21,
            6,
            "leisure",
            "browser",
        ),
    ];
    let baseline = StrongPredictiveActionBaseline::from_training(&examples);
    let query = example_with_context(
        "new-user",
        vec!["unknown".into()],
        "unknown",
        8,
        1,
        "work",
        "mail",
    );

    let pred = baseline.predict_for_example(&query, 2);

    assert_eq!(
        pred[0], "mail",
        "hour/weekday/event context should overcome global browser popularity"
    );
}
