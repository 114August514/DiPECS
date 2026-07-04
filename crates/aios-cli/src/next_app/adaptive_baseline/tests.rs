use super::*;
use aios_agent::NextAppTrainingExample;

fn example(user_id: &str, current_app: &str, label_app: &str) -> NextAppTrainingExample {
    NextAppTrainingExample {
        user_id: user_id.into(),
        current_app: current_app.into(),
        history: vec![],
        hour_bucket: 12,
        weekday: 1,
        event_type: "foreground".into(),
        label_app: label_app.into(),
    }
}

fn example_with_history(
    user_id: &str,
    history: Vec<String>,
    current_app: &str,
    hour: u8,
    weekday: u8,
    label_app: &str,
) -> NextAppTrainingExample {
    NextAppTrainingExample {
        user_id: user_id.into(),
        current_app: current_app.into(),
        history,
        hour_bucket: hour,
        weekday,
        event_type: "foreground".into(),
        label_app: label_app.into(),
    }
}

#[test]
fn order3_markov_uses_two_step_history() {
    // Pattern: X -> A -> B always, but A -> C without X context
    let examples = vec![
        example_with_history("u1", vec!["X".into(), "A".into()], "A", 12, 1, "B"),
        example_with_history("u2", vec!["X".into(), "A".into()], "A", 12, 1, "B"),
        example_with_history("u3", vec!["Y".into(), "A".into()], "A", 12, 1, "C"),
        example_with_history("u4", vec!["Y".into(), "A".into()], "A", 12, 1, "C"),
    ];

    let baseline = AdaptiveBaseline::from_training(&examples);
    let query = example_with_history("new", vec!["X".into(), "A".into()], "A", 12, 1, "B");
    let pred = baseline.predict_for_example(&query, 2);

    assert_eq!(
        pred[0], "B",
        "order-3 Markov should use two-step history X->A->B"
    );
}

#[test]
fn time_markov_prefers_hour_specific_transition() {
    // com.chat -> com.mail in morning, com.chat -> com.browser in evening
    let mut examples = Vec::new();
    for _ in 0..10 {
        examples.push(example_with_history(
            "u1",
            vec![],
            "com.chat",
            8,
            1,
            "com.mail",
        ));
        examples.push(example_with_history(
            "u2",
            vec![],
            "com.chat",
            8,
            1,
            "com.mail",
        ));
    }
    for _ in 0..10 {
        examples.push(example_with_history(
            "u3",
            vec![],
            "com.chat",
            21,
            1,
            "com.browser",
        ));
        examples.push(example_with_history(
            "u4",
            vec![],
            "com.chat",
            21,
            1,
            "com.browser",
        ));
    }

    let baseline = AdaptiveBaseline::from_training(&examples);

    // Morning query should prefer mail
    let morning = example_with_history("new", vec![], "com.chat", 8, 1, "com.mail");
    let pred_morning = baseline.predict_for_example(&morning, 2);
    assert!(
        pred_morning.iter().any(|a| a == "com.mail"),
        "morning query should include com.mail: {pred_morning:?}"
    );

    // Evening query should prefer browser
    let evening = example_with_history("new", vec![], "com.chat", 21, 1, "com.browser");
    let pred_evening = baseline.predict_for_example(&evening, 2);
    assert!(
        pred_evening.iter().any(|a| a == "com.browser"),
        "evening query should include com.browser: {pred_evening:?}"
    );
}

#[test]
fn adaptive_baseline_combines_all_rankers() {
    let mut examples = Vec::new();
    // Consistent pattern: chat -> mail
    for i in 0..20 {
        examples.push(example(&format!("u{}", i % 5), "com.chat", "com.mail"));
    }
    // Consistent pattern: mail -> browser
    for i in 0..20 {
        examples.push(example(&format!("u{}", i % 5), "com.mail", "com.browser"));
    }

    let baseline = AdaptiveBaseline::from_training(&examples);
    let query = example("u1", "com.chat", "com.mail");
    let pred = baseline.predict_for_example(&query, 3);

    assert_eq!(
        pred[0], "com.mail",
        "adaptive baseline should rank com.mail first"
    );
    assert!(!pred.is_empty(), "should produce predictions");
}

#[test]
fn adaptive_baseline_handles_unknown_context() {
    let examples = vec![
        example("u1", "com.chat", "com.mail"),
        example("u1", "com.chat", "com.mail"),
        example("u2", "com.chat", "com.browser"),
    ];

    let baseline = AdaptiveBaseline::from_training(&examples);
    let query = example("unknown", "com.unknown", "com.mail");
    let pred = baseline.predict_for_example(&query, 3);

    assert!(
        !pred.is_empty(),
        "should fall back to popularity for unknown context"
    );
}
