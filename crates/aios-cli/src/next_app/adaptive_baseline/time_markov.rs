//! Context-aware Markov with temporal features.
//!
//! Keys on `(current, hour_bucket) -> next` and `(current, weekday) -> next`
//! to capture time-of-day and day-of-week patterns that global Markov averages
//! over. Blends temporal signals with global Markov using learned weights.

use std::collections::{BTreeMap, HashMap};

use aios_agent::NextAppTrainingExample;

use super::super::strong_baseline::markov::rank_counts;

/// Temporal Markov model blending hour and weekday signals with global Markov.
pub(super) struct TimeMarkov {
    /// Global order-1 Markov: current -> ranked apps.
    global_markov: HashMap<String, Vec<String>>,
    /// Hour-keyed transitions: "{current}\t{hour}" -> ranked apps.
    hour_markov: HashMap<String, Vec<String>>,
    /// Weekday-keyed transitions: "{current}\t{weekday}" -> ranked apps.
    weekday_markov: HashMap<String, Vec<String>>,
    /// Global popularity fallback.
    popularity: Vec<String>,
    /// Learned blend weight for temporal vs global signal.
    /// 0.0 = pure global, 1.0 = pure temporal.
    blend_weight: f32,
}

impl TimeMarkov {
    pub fn from_training(examples: &[NextAppTrainingExample]) -> Self {
        let mut global_counts: HashMap<String, HashMap<String, u32>> = HashMap::new();
        let mut hour_counts: HashMap<String, HashMap<String, u32>> = HashMap::new();
        let mut weekday_counts: HashMap<String, HashMap<String, u32>> = HashMap::new();
        let mut popularity_counts: HashMap<String, u32> = HashMap::new();
        let mut temporal_hit = 0u32;
        let mut temporal_miss = 0u32;

        for example in examples {
            let next = &example.label_app;
            let current = &example.current_app;

            *popularity_counts.entry(next.clone()).or_default() += 1;

            *global_counts
                .entry(current.clone())
                .or_default()
                .entry(next.clone())
                .or_default() += 1;

            let hour_key = format!("{}\t{}", current, example.hour_bucket);
            *hour_counts
                .entry(hour_key)
                .or_default()
                .entry(next.clone())
                .or_default() += 1;

            let weekday_key = format!("{}\t{}", current, example.weekday);
            *weekday_counts
                .entry(weekday_key)
                .or_default()
                .entry(next.clone())
                .or_default() += 1;
        }

        // Compute blend weight from a simple heuristic: how often does the
        // hour-specific top-1 match the actual label vs the global top-1.
        // We use a held-out 10% slice for this.
        let val_size = examples.len() / 10;
        if val_size > 0 {
            for example in examples.iter().take(val_size) {
                let current = &example.current_app;
                let hour_key = format!("{}\t{}", current, example.hour_bucket);

                let hour_top1 = hour_counts
                    .get(&hour_key)
                    .and_then(|counts| rank_counts(counts.clone()).into_iter().next());

                let global_top1 = global_counts
                    .get(current)
                    .and_then(|counts| rank_counts(counts.clone()).into_iter().next());

                if let (Some(ref h), Some(ref g)) = (hour_top1, global_top1) {
                    if *h == example.label_app {
                        temporal_hit += 1;
                    } else if *g == example.label_app {
                        temporal_miss += 1;
                    }
                }
            }
        }

        let total = temporal_hit + temporal_miss;
        let blend_weight = if total > 0 {
            (temporal_hit as f32 / total as f32).clamp(0.1, 0.9)
        } else {
            0.3
        };

        Self {
            global_markov: global_counts
                .into_iter()
                .map(|(k, counts)| (k, rank_counts(counts)))
                .collect(),
            hour_markov: hour_counts
                .into_iter()
                .map(|(k, counts)| (k, rank_counts(counts)))
                .collect(),
            weekday_markov: weekday_counts
                .into_iter()
                .map(|(k, counts)| (k, rank_counts(counts)))
                .collect(),
            popularity: rank_counts(popularity_counts),
            blend_weight,
        }
    }

    /// Predict top-k next apps using temporal + global blended scores.
    pub fn predict(
        &self,
        current_app: &str,
        hour_bucket: u8,
        weekday: u8,
        top_k: usize,
    ) -> Vec<String> {
        let mut scores: BTreeMap<String, f32> = BTreeMap::new();

        // Global Markov signal
        if let Some(ranked) = self.global_markov.get(current_app) {
            for (rank, app) in ranked.iter().enumerate() {
                if app == current_app {
                    continue;
                }
                *scores.entry(app.clone()).or_default() +=
                    (1.0 - self.blend_weight) / (1.0 + rank as f32);
            }
        }

        // Hour signal
        let hour_key = format!("{}\t{}", current_app, hour_bucket);
        if let Some(ranked) = self.hour_markov.get(&hour_key) {
            for (rank, app) in ranked.iter().enumerate() {
                if app == current_app {
                    continue;
                }
                *scores.entry(app.clone()).or_default() +=
                    self.blend_weight * 0.5 / (1.0 + rank as f32);
            }
        }

        // Weekday signal
        let weekday_key = format!("{}\t{}", current_app, weekday);
        if let Some(ranked) = self.weekday_markov.get(&weekday_key) {
            for (rank, app) in ranked.iter().enumerate() {
                if app == current_app {
                    continue;
                }
                *scores.entry(app.clone()).or_default() +=
                    self.blend_weight * 0.5 / (1.0 + rank as f32);
            }
        }

        // Popularity fallback
        for (rank, app) in self.popularity.iter().enumerate() {
            if app == current_app {
                continue;
            }
            *scores.entry(app.clone()).or_default() += 0.01 / (1.0 + rank as f32);
        }

        let mut ranked: Vec<(String, f32)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        ranked.into_iter().map(|(app, _)| app).take(top_k).collect()
    }
}
