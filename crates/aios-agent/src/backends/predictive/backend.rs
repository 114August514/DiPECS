//! PredictiveLocalBackend: DecisionBackend implementation for next-app prediction.

use std::collections::BTreeSet;
use std::time::Instant;

use aios_spec::{
    ActionType, ActionUrgency, AppTransition, DecisionBackendResult, DecisionRoute, Intent,
    IntentBatch, IntentType, ModelInput, RiskLevel, SanitizedEventType, StructuredContext,
    SuggestedAction,
};

use super::{
    hour_bucket, weekday, NextAppAlgorithm, NextAppModelArtifact, NextAppPredictor,
    PredictionFeatures, MAX_BACKEND_INTENTS, MODEL_NAME,
};
use crate::DecisionBackend;

pub struct PredictiveLocalBackend {
    predictor: NextAppPredictor,
}

const POLICY_CONFIDENCE_FLOOR: f32 = 0.30;

impl PredictiveLocalBackend {
    pub fn new(artifact: NextAppModelArtifact) -> Result<Self, String> {
        Ok(Self {
            predictor: NextAppPredictor::new(artifact)?,
        })
    }

    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        Ok(Self {
            predictor: NextAppPredictor::from_path(path)?,
        })
    }
}

impl DecisionBackend for PredictiveLocalBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        let input = ModelInput::current_only(context.clone());
        self.evaluate_model_input(&input)
    }

    fn evaluate_model_input(&self, input: &ModelInput) -> DecisionBackendResult {
        let start = Instant::now();
        let features = features_from_model_input(input);
        let known = known_packages(&input.current_context);
        let predictions =
            self.predictor
                .rank(&features, NextAppAlgorithm::Ensemble, MAX_BACKEND_INTENTS);

        let mut intents: Vec<Intent> = predictions
            .into_iter()
            .map(|prediction| prediction_to_intent(&prediction, &known))
            .collect();

        if intents.is_empty() {
            intents.push(Intent {
                intent_id: crate::new_id(),
                intent_type: IntentType::Idle,
                confidence: 0.50,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::NoOp,
                    target: None,
                    urgency: ActionUrgency::IdleTime,
                }],
                rationale_tags: vec!["predictive:idle_no_prediction".into()],
            });
        }

        let intent_batch = IntentBatch {
            window_id: input.current_context.window_id.clone(),
            intents,
            generated_at_ms: input.current_context.window_end_ms,
            model: MODEL_NAME.into(),
        };
        let rationale_tags = intent_batch
            .intents
            .iter()
            .flat_map(|intent| intent.rationale_tags.iter().cloned())
            .collect();

        DecisionBackendResult {
            route: DecisionRoute::LocalEvaluator,
            intent_batch,
            rationale_tags,
            latency_us: start.elapsed().as_micros() as u64,
            error: None,
        }
    }
}

pub(crate) fn features_from_model_input(input: &ModelInput) -> PredictionFeatures {
    let context = &input.current_context;
    let current_app =
        latest_foreground_app(context).or_else(|| context.summary.foreground_apps.last().cloned());
    let mut history: Vec<String> = input
        .recent_feedback
        .iter()
        .rev()
        .flat_map(|record| record.foreground_apps.iter().rev().cloned())
        .take(5)
        .collect();
    history.reverse();
    let event_type = context.events.last().map(|event| {
        match &event.event_type {
            SanitizedEventType::AppTransition { .. } => "app_transition",
            SanitizedEventType::Notification { .. } => "notification",
            SanitizedEventType::FileActivity { .. } => "file_activity",
            SanitizedEventType::Screen { .. } => "screen",
            SanitizedEventType::SystemStatus { .. } => "system_status",
            SanitizedEventType::ProcessResource { .. } => "process_resource",
            SanitizedEventType::InterAppInteraction { .. } => "inter_app",
        }
        .to_string()
    });
    PredictionFeatures {
        user_id: input.behavior_profile.user_id.clone(),
        current_app,
        history,
        hour_bucket: Some(hour_bucket(context.window_end_ms)),
        weekday: Some(weekday(context.window_end_ms)),
        event_type,
    }
}

fn latest_foreground_app(context: &StructuredContext) -> Option<String> {
    context
        .events
        .iter()
        .rev()
        .find_map(|event| match &event.event_type {
            SanitizedEventType::AppTransition {
                package_name,
                transition: AppTransition::Foreground,
                ..
            } => Some(package_name.clone()),
            _ => None,
        })
}

fn known_packages(context: &StructuredContext) -> BTreeSet<String> {
    let mut packages = BTreeSet::new();
    packages.extend(context.summary.foreground_apps.iter().cloned());
    packages.extend(context.summary.notified_apps.iter().cloned());
    for event in &context.events {
        if let Some(pkg) = &event.app_package {
            packages.insert(pkg.clone());
        }
        match &event.event_type {
            SanitizedEventType::AppTransition { package_name, .. } => {
                packages.insert(package_name.clone());
            },
            SanitizedEventType::Notification { source_package, .. } => {
                packages.insert(source_package.clone());
            },
            SanitizedEventType::ProcessResource {
                package_name: Some(package),
                ..
            } => {
                packages.insert(package.clone());
            },
            _ => {},
        }
    }
    packages
}

/// Map a scored prediction to a policy-safe intent.
///
/// - If the target app is currently in context (foreground/notified/process),
///   we emit `SwitchToApp` with a `PreWarmProcess` action and `Low` risk.
/// - If the target is not currently observed, we emit `OpenApp` with a
///   conservative `KeepAlive` heartbeat action and `Low` risk. The
///   `OpenApp` intent type honestly reflects "we may want to open this app"
///   while the `KeepAlive` action prevents the executor from launching
///   something that is not on the device.
fn prediction_to_intent(prediction: &super::AppScore, known: &BTreeSet<String>) -> Intent {
    let in_context = known.contains(&prediction.app);
    let (intent_type, action_type, target, risk_level) = if in_context {
        (
            IntentType::SwitchToApp(prediction.app.clone()),
            ActionType::PreWarmProcess,
            Some(format!("pkg:{}", prediction.app)),
            RiskLevel::Low,
        )
    } else {
        (
            IntentType::OpenApp(prediction.app.clone()),
            ActionType::KeepAlive,
            Some("work:collector_heartbeat".to_string()),
            RiskLevel::Low,
        )
    };
    Intent {
        intent_id: crate::new_id(),
        intent_type,
        confidence: prediction.score.clamp(POLICY_CONFIDENCE_FLOOR, 0.99),
        risk_level,
        suggested_actions: vec![SuggestedAction {
            action_type,
            target,
            urgency: ActionUrgency::Immediate,
        }],
        rationale_tags: vec![
            "predictive:next_app".into(),
            if in_context {
                "predictive:target_in_context".into()
            } else {
                "predictive:target_not_in_context_safe_keepalive".into()
            },
        ],
    }
}
