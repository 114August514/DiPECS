//! RuleBasedBackend — 规则驱动的意图生成后端。
//!
//! 扫描 `StructuredContext` 中的事件信号（文件通知、Activity 启动、
//! 前台切换、屏幕状态、电量等），生成对应的 `Intent` 列表。

use std::time::Instant;

use aios_spec::{
    ActionType, ActionUrgency, AppTransition, DecisionBackendResult, DecisionRoute, Intent,
    IntentBatch, IntentType, RiskLevel, SanitizedEventType, SemanticHint, StructuredContext,
    SuggestedAction,
};

use super::prefetch_target::default_prefetch_target;
use crate::{new_id, DecisionBackend};

pub struct RuleBasedBackend;

impl RuleBasedBackend {
    /// Generate intents by scanning context events for known signal patterns.
    fn generate_intents(&self, context: &StructuredContext) -> Vec<Intent> {
        let mut intents = Vec::new();
        let summary = &context.summary;

        let mut has_file_mention = false;
        let mut has_activity_launch = false;
        let mut launched_apps: Vec<String> = Vec::new();
        let mut observed_foreground_apps: Vec<String> = Vec::new();
        let mut has_screen_on = false;
        let mut is_low_battery = false;
        let notified_apps: Vec<String> = summary.notified_apps.clone();

        for event in &context.events {
            match &event.event_type {
                SanitizedEventType::Notification { semantic_hints, .. }
                    if semantic_hints.contains(&SemanticHint::FileMention) =>
                {
                    has_file_mention = true;
                },
                SanitizedEventType::InterAppInteraction {
                    interaction_type,
                    source_package,
                    ..
                } => {
                    if matches!(interaction_type, aios_spec::InteractionType::ActivityLaunch) {
                        has_activity_launch = true;
                        if let Some(pkg) = source_package {
                            if !launched_apps.contains(pkg) {
                                launched_apps.push(pkg.clone());
                            }
                        }
                    }
                },
                SanitizedEventType::AppTransition {
                    package_name,
                    transition: AppTransition::Foreground,
                    ..
                } if !observed_foreground_apps.contains(package_name) => {
                    observed_foreground_apps.push(package_name.clone());
                },
                SanitizedEventType::FileActivity {
                    package_name,
                    extension_category,
                    ..
                } => {
                    intents.push(Intent {
                        intent_id: new_id(),
                        intent_type: IntentType::HandleFile(extension_category.clone()),
                        confidence: 0.75,
                        risk_level: RiskLevel::Low,
                        suggested_actions: vec![SuggestedAction {
                            action_type: ActionType::PrefetchFile,
                            target: Some(default_prefetch_target(
                                extension_category,
                                package_name.as_deref(),
                            )),
                            urgency: ActionUrgency::IdleTime,
                        }],
                        rationale_tags: vec![format!("{:?}", extension_category)],
                    });
                },
                SanitizedEventType::Screen { state } => {
                    if matches!(state, aios_spec::ScreenState::Interactive) {
                        has_screen_on = true;
                    }
                },
                SanitizedEventType::SystemStatus {
                    battery_pct: Some(pct),
                    ..
                } if *pct < 20 => {
                    is_low_battery = true;
                },
                _ => {},
            }
        }

        if has_file_mention {
            let from_app = notified_apps.first().cloned().unwrap_or_default();
            let prewarm_target = if from_app.is_empty() {
                "own:resources".to_string()
            } else {
                format!("pkg:{from_app}")
            };
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::OpenApp(from_app.clone()),
                confidence: 0.70,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::PreWarmProcess,
                    target: Some(prewarm_target),
                    urgency: ActionUrgency::Immediate,
                }],
                rationale_tags: vec!["file_received".into()],
            });
        }

        if has_activity_launch && !launched_apps.is_empty() {
            let target = launched_apps[0].clone();
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::SwitchToApp(target.clone()),
                confidence: 0.85,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![
                    SuggestedAction {
                        action_type: ActionType::PreWarmProcess,
                        target: Some(format!("pkg:{target}")),
                        urgency: ActionUrgency::Immediate,
                    },
                    SuggestedAction {
                        action_type: ActionType::KeepAlive,
                        target: Some("work:collector_heartbeat".into()),
                        urgency: ActionUrgency::Immediate,
                    },
                ],
                rationale_tags: vec!["app_launch_detected".into()],
            });
        }

        if let Some(target) = observed_foreground_apps.first().cloned() {
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::SwitchToApp(target.clone()),
                confidence: 0.80,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![
                    SuggestedAction {
                        action_type: ActionType::PreWarmProcess,
                        target: Some(format!("pkg:{target}")),
                        urgency: ActionUrgency::Immediate,
                    },
                    SuggestedAction {
                        action_type: ActionType::KeepAlive,
                        target: Some("work:collector_heartbeat".into()),
                        urgency: ActionUrgency::Immediate,
                    },
                ],
                rationale_tags: vec!["app_foreground_observed".into()],
            });
        }

        if has_screen_on {
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::Idle,
                confidence: 0.60,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::KeepAlive,
                    target: Some("work:collector_heartbeat".into()),
                    urgency: ActionUrgency::IdleTime,
                }],
                rationale_tags: vec!["screen_on".into()],
            });
        }

        if is_low_battery {
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::Idle,
                confidence: 0.80,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::ReleaseMemory,
                    target: Some("cache:prefetch".into()),
                    urgency: ActionUrgency::Immediate,
                }],
                rationale_tags: vec!["low_battery".into()],
            });
        }

        if intents.is_empty() {
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::Idle,
                confidence: 0.50,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::NoOp,
                    target: None,
                    urgency: ActionUrgency::IdleTime,
                }],
                rationale_tags: vec!["idle_window".into()],
            });
        }

        tracing::debug!(
            window_id = %context.window_id,
            event_count = context.events.len(),
            intent_count = intents.len(),
            "RuleBasedBackend generated intents"
        );

        intents
    }
}

impl DecisionBackend for RuleBasedBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        let start = Instant::now();
        let intents = self.generate_intents(context);
        let intent_batch = IntentBatch {
            window_id: context.window_id.clone(),
            intents,
            generated_at_ms: context.window_end_ms,
            model: "rule-based-v0.2".to_string(),
        };
        let rationale_tags = intent_batch
            .intents
            .iter()
            .flat_map(|intent| intent.rationale_tags.iter().cloned())
            .collect();

        DecisionBackendResult {
            route: DecisionRoute::RuleBased,
            intent_batch,
            rationale_tags,
            latency_us: start.elapsed().as_micros() as u64,
            error: None,
        }
    }
}
