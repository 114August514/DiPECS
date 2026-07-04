//! Adaptive predictive baseline combining enhanced statistical models.
//!
//! This baseline improves on `StrongPredictiveActionBaseline` by adding:
//!
//! - **Order-3 Markov** — captures longer usage sequences with backoff chain
//!   (order-3 -> order-2 -> order-1 -> popularity)
//! - **Temporal Markov** — hour-of-day and day-of-week aware transitions
//!
//! It combines these with the existing strong baseline's signals via a simple
//! RRF-style scoring function.

mod markov3;
mod time_markov;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use aios_agent::NextAppTrainingExample;

use markov3::Markov3;
use time_markov::TimeMarkov;

use super::strong_baseline::{ScoredApp, StrongPredictiveActionBaseline};

/// Adaptive baseline combining enhanced statistical models.
pub(crate) struct AdaptiveBaseline {
    strong: StrongPredictiveActionBaseline,
    markov3: Markov3,
    time_markov: TimeMarkov,
    /// Global popularity for fallback.
    popularity: Vec<String>,
}

impl AdaptiveBaseline {
    pub fn from_training(examples: &[NextAppTrainingExample]) -> Self {
        let mut global_counts: HashMap<String, u32> = HashMap::new();
        for example in examples {
            *global_counts.entry(example.label_app.clone()).or_default() += 1;
        }
        let mut popularity: Vec<(String, u32)> = global_counts.into_iter().collect();
        popularity.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        Self {
            strong: StrongPredictiveActionBaseline::from_training(examples),
            markov3: Markov3::from_training(examples),
            time_markov: TimeMarkov::from_training(examples),
            popularity: popularity.into_iter().map(|(app, _)| app).collect(),
        }
    }

    /// Predict top-k next apps using the full adaptive model.
    pub fn predict_for_example(
        &self,
        example: &NextAppTrainingExample,
        top_k: usize,
    ) -> Vec<String> {
        if top_k == 0 {
            return Vec::new();
        }

        let mut candidates: Vec<String> = Vec::new();

        // Collect candidates from all sub-models
        let strong_pred = self.strong.predict_for_example(example, top_k * 2);
        for app in &strong_pred {
            push_unique(&mut candidates, &example.current_app, app.clone());
        }

        let m3_pred = self.markov3.predict(
            &example.user_id,
            &example.current_app,
            &example.history,
            top_k * 2,
        );
        for app in &m3_pred {
            push_unique(&mut candidates, &example.current_app, app.clone());
        }

        let time_pred = self.time_markov.predict(
            &example.current_app,
            example.hour_bucket,
            example.weekday,
            top_k * 2,
        );
        for app in &time_pred {
            push_unique(&mut candidates, &example.current_app, app.clone());
        }

        // Score each candidate using RRF-like scoring from all sub-model rankings
        let mut scored: Vec<ScoredApp> = candidates
            .into_iter()
            .map(|app| {
                let mut score = 0.0_f32;

                // Strong baseline signal (weight 1.0)
                if let Some(pos) = strong_pred.iter().position(|a| a == &app) {
                    score += 1.0 / (1.0 + pos as f32);
                }

                // Order-3 Markov signal (weight 0.8)
                if let Some(pos) = m3_pred.iter().position(|a| a == &app) {
                    score += 0.8 / (1.0 + pos as f32);
                }

                // Temporal Markov signal (weight 0.6)
                if let Some(pos) = time_pred.iter().position(|a| a == &app) {
                    score += 0.6 / (1.0 + pos as f32);
                }

                // Global popularity tiebreaker
                if let Some(pos) = self.popularity.iter().position(|a| a == &app) {
                    score += 0.01 / (1.0 + pos as f32);
                }

                ScoredApp { app, score }
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.app.cmp(&b.app))
        });

        scored
            .into_iter()
            .map(|s| s.app)
            .filter(|app| app != &example.current_app)
            .take(top_k)
            .collect()
    }
}

fn push_unique(candidates: &mut Vec<String>, current_app: &str, app: String) {
    if app == current_app || candidates.contains(&app) {
        return;
    }
    candidates.push(app);
}
