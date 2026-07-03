use std::collections::{BTreeSet, HashMap};

/// Context Naive Bayes model keyed by predicted next app.
#[derive(Debug, Default)]
pub(super) struct ContextBayes {
    class_counts: HashMap<String, u32>,
    feature_counts: HashMap<String, HashMap<String, u32>>,
    class_feature_totals: HashMap<String, u32>,
    vocabulary: BTreeSet<String>,
    total_examples: u32,
}

impl ContextBayes {
    pub(super) fn observe<I>(&mut self, class: &str, features: I)
    where
        I: IntoIterator<Item = String>,
    {
        *self.class_counts.entry(class.to_string()).or_default() += 1;
        self.total_examples += 1;

        for feature in features {
            self.vocabulary.insert(feature.clone());
            *self
                .feature_counts
                .entry(class.to_string())
                .or_default()
                .entry(feature)
                .or_default() += 1;
            *self
                .class_feature_totals
                .entry(class.to_string())
                .or_default() += 1;
        }
    }

    pub(super) fn rank(&self, features: &[String]) -> Vec<String> {
        if self.total_examples == 0 {
            return Vec::new();
        }

        let class_space = self.class_counts.len().max(1) as f64;
        let vocabulary_size = self.vocabulary.len().max(1) as f64;
        let mut scored = Vec::new();

        for class in self.class_counts.keys() {
            let class_count = *self.class_counts.get(class).unwrap_or(&0) as f64;
            let mut log_score =
                ((class_count + 1.0) / (self.total_examples as f64 + class_space)).ln();
            let class_feature_total = *self.class_feature_totals.get(class).unwrap_or(&0) as f64;
            let feature_counts = self.feature_counts.get(class);

            for feature in features {
                if !self.vocabulary.contains(feature) {
                    continue;
                }
                let count = feature_counts
                    .and_then(|counts| counts.get(feature))
                    .copied()
                    .unwrap_or(0) as f64;
                log_score += ((count + 1.0) / (class_feature_total + vocabulary_size)).ln();
            }
            scored.push((class.clone(), log_score));
        }

        scored.sort_by(|(app_a, score_a), (app_b, score_b)| {
            score_b.total_cmp(score_a).then_with(|| app_a.cmp(app_b))
        });
        scored.into_iter().map(|(app, _)| app).collect()
    }
}
