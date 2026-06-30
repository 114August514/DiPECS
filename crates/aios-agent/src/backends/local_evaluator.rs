//! LocalEvaluatorBackend - deterministic local intent evaluator.
//!
//! This backend is the local-only middle tier between hard-coded rules and a
//! cloud LLM. It deliberately avoids network access and raw event data. The
//! current implementation is a lightweight scorer over `StructuredContext`
//! signals, producing only low-risk actions allowed by the LocalEvaluator
//! capability profile.

use std::time::Instant;

use aios_spec::{
    ActionType, ActionUrgency, AppTransition, DecisionBackendResult, DecisionRoute, Intent,
    IntentBatch, IntentType, RiskLevel, SanitizedEventType, SemanticHint, StructuredContext,
    SuggestedAction,
};

use super::prefetch_target::default_prefetch_target;
use crate::{new_id, DecisionBackend};

pub struct LocalEvaluatorBackend;

impl LocalEvaluatorBackend {
    fn evaluate_intents(&self, context: &StructuredContext) -> Vec<Intent> {
        let mut intents = Vec::new();

        for event in &context.events {
            match &event.event_type {
                SanitizedEventType::FileActivity {
                    package_name,
                    extension_category,
                    is_hot_file,
                    ..
                } => {
                    let confidence = if *is_hot_file { 0.86 } else { 0.78 };
                    intents.push(Intent {
                        intent_id: new_id(),
                        intent_type: IntentType::HandleFile(extension_category.clone()),
                        confidence,
                        risk_level: RiskLevel::Low,
                        suggested_actions: vec![SuggestedAction {
                            action_type: ActionType::PrefetchFile,
                            target: Some(default_prefetch_target(
                                extension_category,
                                package_name.as_deref(),
                            )),
                            urgency: ActionUrgency::IdleTime,
                        }],
                        rationale_tags: vec![
                            "local:file_activity".into(),
                            format!("local:extension:{extension_category:?}"),
                        ],
                    });
                },
                SanitizedEventType::Notification {
                    source_package,
                    semantic_hints,
                    is_ongoing,
                    ..
                } if semantic_hints.contains(&SemanticHint::FileMention)
                    || semantic_hints.contains(&SemanticHint::ImageMention)
                    || semantic_hints.contains(&SemanticHint::LinkAttachment) =>
                {
                    let confidence = if *is_ongoing { 0.66 } else { 0.74 };
                    intents.push(Intent {
                        intent_id: new_id(),
                        intent_type: IntentType::OpenApp(source_package.clone()),
                        confidence,
                        risk_level: RiskLevel::Low,
                        suggested_actions: vec![SuggestedAction {
                            action_type: ActionType::PreWarmProcess,
                            target: Some(format!("pkg:{source_package}")),
                            urgency: ActionUrgency::Immediate,
                        }],
                        rationale_tags: vec!["local:attachment_notification".into()],
                    });
                },
                SanitizedEventType::AppTransition {
                    package_name,
                    transition: AppTransition::Foreground,
                    ..
                } => {
                    intents.push(Intent {
                        intent_id: new_id(),
                        intent_type: IntentType::SwitchToApp(package_name.clone()),
                        confidence: 0.82,
                        risk_level: RiskLevel::Low,
                        suggested_actions: vec![
                            SuggestedAction {
                                action_type: ActionType::PreWarmProcess,
                                target: Some(format!("pkg:{package_name}")),
                                urgency: ActionUrgency::Immediate,
                            },
                            SuggestedAction {
                                action_type: ActionType::KeepAlive,
                                target: Some("work:collector_heartbeat".into()),
                                urgency: ActionUrgency::Immediate,
                            },
                        ],
                        rationale_tags: vec!["local:foreground_transition".into()],
                    });
                },
                SanitizedEventType::InterAppInteraction {
                    source_package,
                    interaction_type:
                        aios_spec::InteractionType::ActivityLaunch
                        | aios_spec::InteractionType::ShareIntent,
                    ..
                } => {
                    let target = source_package
                        .clone()
                        .unwrap_or_else(|| "own:resources".to_string());
                    let action_target = if target.starts_with("own:") {
                        target.clone()
                    } else {
                        format!("pkg:{target}")
                    };
                    intents.push(Intent {
                        intent_id: new_id(),
                        intent_type: IntentType::SwitchToApp(target),
                        confidence: 0.80,
                        risk_level: RiskLevel::Low,
                        suggested_actions: vec![SuggestedAction {
                            action_type: ActionType::PreWarmProcess,
                            target: Some(action_target),
                            urgency: ActionUrgency::Immediate,
                        }],
                        rationale_tags: vec!["local:inter_app_interaction".into()],
                    });
                },
                SanitizedEventType::SystemStatus {
                    battery_pct: Some(pct),
                    ..
                } if *pct < 20 => {
                    intents.push(Intent {
                        intent_id: new_id(),
                        intent_type: IntentType::Idle,
                        confidence: 0.84,
                        risk_level: RiskLevel::Low,
                        suggested_actions: vec![SuggestedAction {
                            action_type: ActionType::ReleaseMemory,
                            target: Some("cache:prefetch".into()),
                            urgency: ActionUrgency::Immediate,
                        }],
                        rationale_tags: vec!["local:low_battery".into()],
                    });
                },
                _ => {},
            }
        }

        if intents.is_empty() {
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::Idle,
                confidence: 0.60,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::NoOp,
                    target: None,
                    urgency: ActionUrgency::IdleTime,
                }],
                rationale_tags: vec!["local:idle_window".into()],
            });
        }

        intents
    }
}

impl DecisionBackend for LocalEvaluatorBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        let start = Instant::now();
        let intents = self.evaluate_intents(context);
        let intent_batch = IntentBatch {
            window_id: context.window_id.clone(),
            intents,
            generated_at_ms: context.window_end_ms,
            model: "local-evaluator-v0.1".into(),
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
