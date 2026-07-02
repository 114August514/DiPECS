//! Adapt DiPECS decision backends into the `NextAppPredictor` trait.

use aios_agent::{DecisionBackend, LocalEvaluatorBackend, RuleBasedBackend};
use aios_spec::{Intent, IntentType, StructuredContext};

use super::types::{NextAppPredictor, PredictionResult, ScoredPrediction};

pub struct RuleBasedNextAppBackend;

impl NextAppPredictor for RuleBasedNextAppBackend {
    fn name(&self) -> &'static str {
        "rule_based"
    }

    fn predict(
        &self,
        ctx: &StructuredContext,
        _current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let decision = RuleBasedBackend.evaluate(ctx);
        PredictionResult {
            ranked: extract_ranked_apps(&decision.intent_batch.intents, candidates),
            latency_us: decision.latency_us,
            rationale_present: decision
                .intent_batch
                .intents
                .iter()
                .any(|i| !i.rationale_tags.is_empty()),
        }
    }
}

pub struct LocalEvaluatorNextAppBackend;

impl NextAppPredictor for LocalEvaluatorNextAppBackend {
    fn name(&self) -> &'static str {
        "local_evaluator"
    }

    fn predict(
        &self,
        ctx: &StructuredContext,
        _current_app: &str,
        candidates: &[String],
    ) -> PredictionResult {
        let decision = LocalEvaluatorBackend.evaluate(ctx);
        PredictionResult {
            ranked: extract_ranked_apps(&decision.intent_batch.intents, candidates),
            latency_us: decision.latency_us,
            rationale_present: decision
                .intent_batch
                .intents
                .iter()
                .any(|i| !i.rationale_tags.is_empty()),
        }
    }
}

fn extract_ranked_apps(intents: &[Intent], candidates: &[String]) -> Vec<ScoredPrediction> {
    let mut best: std::collections::HashMap<String, f32> = std::collections::HashMap::new();

    for intent in intents {
        let pkg = match &intent.intent_type {
            IntentType::OpenApp(p)
            | IntentType::SwitchToApp(p)
            | IntentType::CheckNotification(p) => p.clone(),
            IntentType::HandleFile(_) | IntentType::EnterContext(_) | IntentType::Idle => continue,
        };
        best.entry(pkg)
            .and_modify(|s| *s = s.max(intent.confidence))
            .or_insert(intent.confidence);
    }

    let mut ranked: Vec<ScoredPrediction> = best
        .into_iter()
        .map(|(package, score)| ScoredPrediction { package, score })
        .collect();
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.package.cmp(&b.package))
    });

    ranked
        .into_iter()
        .filter(|p| candidates.contains(&p.package))
        .collect()
}
