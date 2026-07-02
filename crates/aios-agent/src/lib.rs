//! # aios-agent - decision routing and model backends
//!
//! Receives `StructuredContext`, selects a local rule/local evaluator/cloud
//! backend, and returns an `IntentBatch` for final review in core.

mod backends;
mod router;

pub use backends::cloud_llm::ProfileSummarizer;
pub use backends::fallback::FallbackNoOpBackend;
pub use backends::local_evaluator::LocalEvaluatorBackend;
pub use backends::predictive::{
    prediction_features_for_example, train_next_app_artifact, NextAppAlgorithm,
    NextAppModelArtifact, NextAppModelConfig, NextAppPredictor, NextAppTrainingExample,
    PredictionFeatures, PredictiveLocalBackend,
};
pub use backends::rule_based::RuleBasedBackend;
pub use router::{DecisionRouter, RouterConfig};

use aios_spec::{DecisionBackendResult, ModelInput, StructuredContext};
use uuid::Uuid;

// ============================================================
// DecisionBackend trait
// ============================================================

/// Common backend interface: receive a context and return a decision result.
///
/// Rule-based, local evaluator, cloud LLM, and fallback backends all implement
/// this trait.
pub trait DecisionBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult;

    fn evaluate_model_input(&self, input: &ModelInput) -> DecisionBackendResult {
        self.evaluate(&input.current_context)
    }
}

// ============================================================
// Helpers
// ============================================================

fn new_id() -> String {
    Uuid::new_v4().to_string()
}
