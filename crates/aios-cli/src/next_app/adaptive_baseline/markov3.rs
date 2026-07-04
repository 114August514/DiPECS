//! Higher-order Markov (order-3) with backoff chain.
//!
//! Keys on `(prev2, prev1, current) -> next` with fallback to order-2,
//! then order-1, then global popularity.

use std::collections::HashMap;

use aios_agent::NextAppTrainingExample;

use super::super::strong_baseline::markov::rank_counts;

use super::super::strong_baseline::markov::previous_app;

/// Order-3 Markov model with backoff chain.
pub(super) struct Markov3 {
    /// Pre-ranked order-3 candidates: (prev2, prev1, current) -> ranked apps.
    rankings: HashMap<(String, String, String), Vec<String>>,
    /// Pre-ranked order-2 candidates: (prev1, current) -> ranked apps.
    order2_rankings: HashMap<(String, String), Vec<String>>,
    /// Pre-ranked order-1 candidates: current -> ranked apps.
    order1_rankings: HashMap<String, Vec<String>>,
    /// Global popularity fallback.
    popularity: Vec<String>,
}

impl Markov3 {
    pub fn from_training(examples: &[NextAppTrainingExample]) -> Self {
        let mut order3_counts: HashMap<(String, String, String), HashMap<String, u32>> =
            HashMap::new();
        let mut order2_counts: HashMap<(String, String), HashMap<String, u32>> = HashMap::new();
        let mut order1_counts: HashMap<String, HashMap<String, u32>> = HashMap::new();
        let mut global_counts: HashMap<String, u32> = HashMap::new();

        for example in examples {
            let next = &example.label_app;
            let current = &example.current_app;

            *global_counts.entry(next.clone()).or_default() += 1;

            *order1_counts
                .entry(current.clone())
                .or_default()
                .entry(next.clone())
                .or_default() += 1;

            if let Some(prev1) = previous_app(example) {
                *order2_counts
                    .entry((prev1.to_string(), current.clone()))
                    .or_default()
                    .entry(next.clone())
                    .or_default() += 1;

                // Order-3: find the second-to-last distinct app in history
                // that is not current and not prev1.
                if let Some(prev2) = second_previous_app(example, prev1) {
                    *order3_counts
                        .entry((prev2.to_string(), prev1.to_string(), current.clone()))
                        .or_default()
                        .entry(next.clone())
                        .or_default() += 1;
                }
            }
        }

        Self {
            rankings: order3_counts
                .into_iter()
                .map(|(k, counts)| (k, rank_counts(counts)))
                .collect(),
            order2_rankings: order2_counts
                .into_iter()
                .map(|(k, counts)| (k, rank_counts(counts)))
                .collect(),
            order1_rankings: order1_counts
                .into_iter()
                .map(|(k, counts)| (k, rank_counts(counts)))
                .collect(),
            popularity: rank_counts(global_counts),
        }
    }

    /// Predict top-k next apps with order-3 -> order-2 -> order-1 backoff.
    pub fn predict(
        &self,
        _user_id: &str,
        current_app: &str,
        history: &[String],
        top_k: usize,
    ) -> Vec<String> {
        let prev1 = history
            .iter()
            .rev()
            .find(|app| app.as_str() != current_app)
            .map(String::as_str);

        // Try order-3
        if let Some(p1) = prev1 {
            if let Some(p2) = find_second_prev(history, current_app, p1) {
                let key = (p2.to_string(), p1.to_string(), current_app.to_string());
                if let Some(ranked) = self.rankings.get(&key) {
                    let result = ranked
                        .iter()
                        .filter(|a| a.as_str() != current_app)
                        .take(top_k)
                        .cloned()
                        .collect::<Vec<_>>();
                    if !result.is_empty() {
                        return result;
                    }
                }
            }
        }

        // Try order-2
        if let Some(p1) = prev1 {
            let key = (p1.to_string(), current_app.to_string());
            if let Some(ranked) = self.order2_rankings.get(&key) {
                let result = ranked
                    .iter()
                    .filter(|a| a.as_str() != current_app)
                    .take(top_k)
                    .cloned()
                    .collect::<Vec<_>>();
                if !result.is_empty() {
                    return result;
                }
            }
        }

        // Try order-1
        if let Some(ranked) = self.order1_rankings.get(current_app) {
            let result = ranked
                .iter()
                .filter(|a| a.as_str() != current_app)
                .take(top_k)
                .cloned()
                .collect::<Vec<_>>();
            if !result.is_empty() {
                return result;
            }
        }

        // Popularity fallback
        self.popularity
            .iter()
            .filter(|a| a.as_str() != current_app)
            .take(top_k)
            .cloned()
            .collect()
    }
}

/// Find the second previous distinct app for order-3 keying.
fn second_previous_app<'a>(
    example: &'a NextAppTrainingExample,
    first_prev: &str,
) -> Option<&'a str> {
    example
        .history
        .iter()
        .rev()
        .find(|app| app.as_str() != example.current_app && app.as_str() != first_prev)
        .map(String::as_str)
}

/// Find the second previous app from a history slice (runtime).
fn find_second_prev<'a>(
    history: &'a [String],
    current_app: &str,
    first_prev: &str,
) -> Option<&'a str> {
    history
        .iter()
        .rev()
        .find(|app| app.as_str() != current_app && app.as_str() != first_prev)
        .map(String::as_str)
}
