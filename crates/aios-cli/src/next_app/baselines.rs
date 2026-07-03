//! Non-personalized baseline rankers (global popularity and per-user frequency).

use std::collections::{BTreeMap, HashMap};

use aios_agent::NextAppTrainingExample;

pub(crate) struct BaselineTables {
    pub global_popularity: Vec<String>,
    user_frequency: BTreeMap<String, Vec<String>>,
}

impl BaselineTables {
    pub fn from_training(examples: &[NextAppTrainingExample]) -> Self {
        let mut global_counts: HashMap<String, u32> = HashMap::new();
        let mut user_counts: BTreeMap<String, HashMap<String, u32>> = BTreeMap::new();
        for example in examples {
            *global_counts.entry(example.label_app.clone()).or_default() += 1;
            *user_counts
                .entry(example.user_id.clone())
                .or_default()
                .entry(example.label_app.clone())
                .or_default() += 1;
        }
        Self {
            global_popularity: rank_counts(global_counts),
            user_frequency: user_counts
                .into_iter()
                .map(|(user, counts)| (user, rank_counts(counts)))
                .collect(),
        }
    }

    pub fn mfu(&self, user_id: &str) -> Vec<String> {
        self.user_frequency
            .get(user_id)
            .cloned()
            .unwrap_or_else(|| self.global_popularity.clone())
            .into_iter()
            .take(5)
            .collect()
    }
}

pub(crate) fn rank_counts(counts: HashMap<String, u32>) -> Vec<String> {
    let mut ranked: Vec<(String, u32)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.into_iter().map(|(app, _)| app).collect()
}
