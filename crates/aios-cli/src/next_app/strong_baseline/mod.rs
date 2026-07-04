//! Strong predictive baseline for next-app prediction.
//!
//! This is **not** a toy rule; it combines multiple strong signals available to
//! any Android app with usage-stats access:
//!
//! - Markov transitions (P(next | current), with order-2 backoff when history is available)
//! - Per-user frequency / MFU
//! - Context Naive Bayes over hour / weekday / event / short history
//! - Recency (last used app)
//! - Global popularity fallback
//!
//! It is intentionally deterministic and lightweight so it can serve as a fair
//! `StrongPredictiveActionBaseline` in evaluation. The rankers here do **not**
//! contain hard-coded action-value constants; they only output a ranked list of
//! predicted next apps.

mod bayes;
pub(crate) mod markov;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};

use aios_agent::NextAppTrainingExample;
use bayes::ContextBayes;
use markov::{context_features, previous_app, rank_counts, rank_markov_counts, rank_order2_counts};

/// A scored app prediction.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScoredApp {
    pub app: String,
    pub score: f32,
}

/// Strong predictive baseline combining Markov, frequency and recency.
pub(crate) struct StrongPredictiveActionBaseline {
    /// Global popularity ranking.
    global_popularity: Vec<String>,
    /// Per-user frequency ranking.
    user_frequency: BTreeMap<String, Vec<String>>,
    /// Markov transition counts: (current_app, next_app) -> count.
    markov: HashMap<(String, String), u32>,
    /// Total transitions per current_app.
    markov_totals: HashMap<String, u32>,
    /// Pre-ranked first-order Markov candidates by current_app.
    markov_rankings: HashMap<String, Vec<String>>,
    /// Pre-ranked second-order Markov candidates by (previous_app, current_app).
    markov_order2_rankings: HashMap<(String, String), Vec<String>>,
    /// Recency table: (user_id, current_app) -> most recent next_app.
    recency: HashMap<(String, String), String>,
    /// Context Naive Bayes model.
    bayes: ContextBayes,
}

impl StrongPredictiveActionBaseline {
    pub fn from_training(examples: &[NextAppTrainingExample]) -> Self {
        let mut global_counts: HashMap<String, u32> = HashMap::new();
        let mut user_counts: BTreeMap<String, HashMap<String, u32>> = BTreeMap::new();
        let mut markov: HashMap<(String, String), u32> = HashMap::new();
        let mut markov_totals: HashMap<String, u32> = HashMap::new();
        let mut markov_order2: HashMap<(String, String, String), u32> = HashMap::new();
        let mut recency: HashMap<(String, String), String> = HashMap::new();
        let mut bayes = ContextBayes::default();

        for example in examples {
            let next_app = &example.label_app;
            let current = &example.current_app;
            let user = &example.user_id;

            // Global / per-user frequency.
            *global_counts.entry(next_app.clone()).or_default() += 1;
            *user_counts
                .entry(user.clone())
                .or_default()
                .entry(next_app.clone())
                .or_default() += 1;

            // Markov transitions.
            *markov
                .entry((current.clone(), next_app.clone()))
                .or_default() += 1;
            *markov_totals.entry(current.clone()).or_default() += 1;
            if let Some(previous) = previous_app(example) {
                *markov_order2
                    .entry((previous.to_string(), current.clone(), next_app.clone()))
                    .or_default() += 1;
            }

            // Recency: keep the most recent observed next_app per (user, current_app).
            // The training examples are assumed to be in chronological order per user.
            recency.insert((user.clone(), current.clone()), next_app.clone());

            bayes.observe(next_app, context_features(example));
        }

        Self {
            global_popularity: rank_counts(global_counts),
            user_frequency: user_counts
                .into_iter()
                .map(|(user, counts)| (user, rank_counts(counts)))
                .collect(),
            markov_rankings: rank_markov_counts(&markov),
            markov_order2_rankings: rank_order2_counts(&markov_order2),
            markov,
            markov_totals,
            recency,
            bayes,
        }
    }

    /// Predict top-k next apps using the full LSApp-shaped training example context.
    pub fn predict_for_example(
        &self,
        example: &NextAppTrainingExample,
        top_k: usize,
    ) -> Vec<String> {
        let features = context_features(example);
        self.predict_with_context(
            &example.user_id,
            &example.current_app,
            previous_app(example),
            &features,
            top_k,
        )
    }

    fn predict_with_context(
        &self,
        user_id: &str,
        current_app: &str,
        previous_app: Option<&str>,
        features: &[String],
        top_k: usize,
    ) -> Vec<String> {
        if top_k == 0 {
            return Vec::new();
        }

        let order2_ranked = previous_app
            .and_then(|previous| {
                self.markov_order2_rankings
                    .get(&(previous.to_string(), current_app.to_string()))
            })
            .cloned()
            .unwrap_or_default();

        let mut candidates: Vec<String> = Vec::new();

        for app in &order2_ranked {
            push_candidate(&mut candidates, current_app, app.clone());
        }

        if let Some(markov_ranked) = self.markov_rankings.get(current_app) {
            for app in markov_ranked {
                push_candidate(&mut candidates, current_app, app.clone());
            }
        }

        if let Some(freq) = self.user_frequency.get(user_id) {
            for app in freq {
                push_candidate(&mut candidates, current_app, app.clone());
            }
        }

        if let Some(app) = self
            .recency
            .get(&(user_id.to_string(), current_app.to_string()))
        {
            push_candidate(&mut candidates, current_app, app.clone());
        }

        let bayes_ranked = self.bayes.rank(features);
        for app in &bayes_ranked {
            push_candidate(&mut candidates, current_app, app.clone());
        }

        for app in &self.global_popularity {
            push_candidate(&mut candidates, current_app, app.clone());
        }

        let mut scored: Vec<ScoredApp> = candidates
            .into_iter()
            .map(|app| {
                self.score_candidate(user_id, current_app, &order2_ranked, &bayes_ranked, app)
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.app.cmp(&b.app))
        });

        scored.into_iter().map(|s| s.app).take(top_k).collect()
    }

    fn score_candidate(
        &self,
        user_id: &str,
        current_app: &str,
        order2_ranked: &[String],
        bayes_ranked: &[String],
        app: String,
    ) -> ScoredApp {
        let mut score = 0.0_f32;

        if let Some(pos) = order2_ranked.iter().position(|ranked| ranked == &app) {
            score += 1.2 / (1.0 + pos as f32);
        }

        if let Some(&count) = self.markov.get(&(current_app.to_string(), app.clone())) {
            if let Some(&total) = self.markov_totals.get(current_app) {
                if total > 0 {
                    score += 0.8 * (count as f32) / (total as f32);
                }
            }
        }

        if let Some(freq) = self.user_frequency.get(user_id) {
            if let Some(pos) = freq.iter().position(|a| a == &app) {
                score += 0.45 / (1.0 + pos as f32);
            }
        }

        if let Some(pos) = bayes_ranked.iter().position(|ranked| ranked == &app) {
            score += 0.6 / (1.0 + pos as f32);
        }

        if self
            .recency
            .get(&(user_id.to_string(), current_app.to_string()))
            .map(|a| a == &app)
            .unwrap_or(false)
        {
            score += 0.25;
        }

        if let Some(pos) = self.global_popularity.iter().position(|a| a == &app) {
            score += 0.05 / (1.0 + pos as f32);
        }

        ScoredApp { app, score }
    }

    /// Convenience accessor for the global popularity fallback.
    #[allow(dead_code)]
    pub fn global_popularity(&self, top_k: usize) -> Vec<String> {
        self.global_popularity.iter().take(top_k).cloned().collect()
    }
}

fn push_candidate(candidates: &mut Vec<String>, current_app: &str, app: String) {
    if app == current_app || candidates.contains(&app) {
        return;
    }
    candidates.push(app);
}
