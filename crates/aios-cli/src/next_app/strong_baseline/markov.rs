use std::collections::HashMap;

use aios_agent::NextAppTrainingExample;

pub(crate) fn previous_app(example: &NextAppTrainingExample) -> Option<&str> {
    example
        .history
        .iter()
        .rev()
        .find(|app| app.as_str() != example.current_app)
        .map(String::as_str)
}

pub(super) fn context_features(example: &NextAppTrainingExample) -> Vec<String> {
    let mut features = Vec::new();
    features.push(format!("current={}", example.current_app));
    if let Some(prev) = previous_app(example) {
        features.push(format!("prev={prev}"));
    }
    for (idx, app) in example.history.iter().rev().take(3).enumerate() {
        features.push(format!("hist{idx}={app}"));
    }
    features.push(format!("hour={}", example.hour_bucket));
    features.push(format!("weekday={}", example.weekday));
    features.push(format!("event={}", example.event_type));
    features
}

pub(crate) fn rank_counts(counts: HashMap<String, u32>) -> Vec<String> {
    let mut ranked: Vec<(String, u32)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.into_iter().map(|(app, _)| app).collect()
}

pub(super) fn rank_markov_counts(
    markov: &HashMap<(String, String), u32>,
) -> HashMap<String, Vec<String>> {
    let mut grouped: HashMap<String, HashMap<String, u32>> = HashMap::new();
    for ((current, next), count) in markov {
        grouped
            .entry(current.clone())
            .or_default()
            .insert(next.clone(), *count);
    }
    grouped
        .into_iter()
        .map(|(current, counts)| (current, rank_counts(counts)))
        .collect()
}

pub(super) fn rank_order2_counts(
    markov_order2: &HashMap<(String, String, String), u32>,
) -> HashMap<(String, String), Vec<String>> {
    let mut grouped: HashMap<(String, String), HashMap<String, u32>> = HashMap::new();
    for ((previous, current, next), count) in markov_order2 {
        grouped
            .entry((previous.clone(), current.clone()))
            .or_default()
            .insert(next.clone(), *count);
    }
    grouped
        .into_iter()
        .map(|(key, counts)| (key, rank_counts(counts)))
        .collect()
}
