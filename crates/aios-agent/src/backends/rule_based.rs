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
                // FileActivity is intentionally not actioned here. Its only
                // useful action is PrefetchFile, which the RuleBased capability
                // forbids — speculative file IO belongs to the richer
                // LocalEvaluator / CloudLlm tier (which do allow PrefetchFile).
                // Emitting it here would only produce perpetual capability
                // denials in the audit log.
                _ => {},
            }
        }

        if has_file_mention {
            let from_app = notified_apps.first().cloned().unwrap_or_default();
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::OpenApp(from_app.clone()),
                confidence: 0.70,
                risk_level: RiskLevel::Low,
                // KeepAlive, not PreWarmProcess: the file-bearing app already
                // exists (it raised the notification), and the RuleBased
                // capability forbids PreWarmProcess. Keeping it warm makes the
                // imminent open fast without a capability-denied pre-warm.
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::KeepAlive,
                    target: Some(from_app),
                    urgency: ActionUrgency::Immediate,
                }],
                rationale_tags: vec!["file_received".into()],
            });
        }

        // Generic notification engagement: an app raised or updated a
        // notification without a file mention. The process usually already
        // exists (it posted the notification) and the user may open it soon,
        // so keep it warm. KeepAlive is deliberate — the RuleBased capability
        // authorizes KeepAlive but denies PreWarmProcess, so a pre-warm here
        // would be dropped by policy. Distinguishing a tap from a dismiss
        // needs the interaction action preserved through the air-gap, which it
        // currently is not (future refinement).
        if !has_file_mention {
            if let Some(app) = notified_apps.first().cloned() {
                intents.push(Intent {
                    intent_id: new_id(),
                    intent_type: IntentType::OpenApp(app.clone()),
                    confidence: 0.55,
                    risk_level: RiskLevel::Low,
                    suggested_actions: vec![SuggestedAction {
                        action_type: ActionType::KeepAlive,
                        target: Some(app),
                        urgency: ActionUrgency::IdleTime,
                    }],
                    rationale_tags: vec!["notification_engagement".into()],
                });
            }
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
                        target: Some(target.clone()),
                        urgency: ActionUrgency::Immediate,
                    },
                    SuggestedAction {
                        action_type: ActionType::KeepAlive,
                        target: Some(target),
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
                // KeepAlive only: PreWarmProcess is outside the RuleBased
                // capability and was previously emitted just to be denied. The
                // app is already foregrounded, so keeping it alive is the
                // meaningful (and authorized) action.
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::KeepAlive,
                    target: Some(target),
                    urgency: ActionUrgency::Immediate,
                }],
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
                    target: summary.foreground_apps.first().cloned(),
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
                    target: None,
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
            model: "rule-based-v0.3".to_string(),
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
