//! Evaluation metrics for next-app rankers.

use std::collections::BTreeMap;

use aios_agent::NextAppTrainingExample;

use super::{MetricsReport, UserAccum};

#[derive(Debug, Default, Clone)]
pub(crate) struct MetricsAccum {
    pub examples: usize,
    pub predicted: usize,
    pub hit1: usize,
    pub hit3: usize,
    pub hit5: usize,
    pub mrr5: f32,
    pub per_user: BTreeMap<String, UserAccum>,
}

pub(crate) fn evaluate_ranker<F>(
    examples: &[NextAppTrainingExample],
    mut ranker: F,
) -> MetricsReport
where
    F: FnMut(&NextAppTrainingExample) -> Vec<String>,
{
    let mut accum = MetricsAccum::default();
    for example in examples {
        accum.examples += 1;
        let predictions = ranker(example);
        if !predictions.is_empty() {
            accum.predicted += 1;
        }
        let rank = predictions
            .iter()
            .take(5)
            .position(|app| app == &example.label_app)
            .map(|idx| idx + 1);
        if rank == Some(1) {
            accum.hit1 += 1;
        }
        if rank.is_some_and(|rank| rank <= 3) {
            accum.hit3 += 1;
        }
        if let Some(rank) = rank {
            accum.hit5 += 1;
            accum.mrr5 += 1.0 / rank as f32;
        }
        let user = accum.per_user.entry(example.user_id.clone()).or_default();
        user.examples += 1;
        if rank == Some(1) {
            user.hit1 += 1;
        }
    }
    accum.into_report()
}

impl MetricsAccum {
    fn into_report(self) -> MetricsReport {
        let denom = self.examples.max(1) as f32;
        let macro_hit_rate_at_1_pct = if self.per_user.is_empty() {
            0.0
        } else {
            self.per_user
                .values()
                .map(|user| user.hit1 as f32 / user.examples.max(1) as f32)
                .sum::<f32>()
                / self.per_user.len() as f32
                * 100.0
        };
        MetricsReport {
            examples: self.examples,
            predicted: self.predicted,
            hit_rate_at_1_pct: pct(self.hit1, denom),
            hit_rate_at_3_pct: pct(self.hit3, denom),
            hit_rate_at_5_pct: pct(self.hit5, denom),
            mean_reciprocal_rank_at_5: round3(self.mrr5 / denom),
            prediction_coverage_pct: pct(self.predicted, denom),
            macro_hit_rate_at_1_pct: round3(macro_hit_rate_at_1_pct),
        }
    }
}

fn pct(numerator: usize, denom: f32) -> f32 {
    round3(numerator as f32 / denom * 100.0)
}

fn round3(value: f32) -> f32 {
    (value * 1000.0).round() / 1000.0
}
