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

/// A single process holding at least this much resident memory (MB) is a heavy
/// footprint whose non-critical caches are worth trimming. Tunable heuristic —
/// validate against real ProcReader traces (Tier B) before treating it as
/// load-bearing.
const MEMORY_PRESSURE_RSS_MB: u32 = 1024;
/// A process with at least this much swapped-out memory (MB) signals the system
/// has been under real memory pressure (zram eviction), independent of its RSS.
const MEMORY_PRESSURE_SWAP_MB: u32 = 128;

impl RuleBasedBackend {
    /// Generate intents by scanning context events for known signal patterns.
    fn generate_intents(&self, context: &StructuredContext) -> Vec<Intent> {
        let mut intents = Vec::new();
        let summary = &context.summary;

        let mut has_file_mention = false;
        let mut observed_foreground_apps: Vec<String> = Vec::new();
        let mut has_screen_on = false;
        let mut is_low_battery = false;
        // The most memory-heavy process seen this window that crosses the
        // pressure threshold: (rss_mb, package). We keep the worst offender.
        let mut memory_pressure: Option<(u32, Option<String>)> = None;
        let notified_apps: Vec<String> = summary.notified_apps.clone();

        for event in &context.events {
            match &event.event_type {
                SanitizedEventType::Notification { semantic_hints, .. }
                    if semantic_hints.contains(&SemanticHint::FileMention) =>
                {
                    has_file_mention = true;
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
                SanitizedEventType::ProcessResource {
                    package_name,
                    vm_rss_mb,
                    vm_swap_mb,
                    ..
                } if *vm_rss_mb >= MEMORY_PRESSURE_RSS_MB
                    || *vm_swap_mb >= MEMORY_PRESSURE_SWAP_MB =>
                {
                    // Keep the heaviest-RSS offender so we trim the worst one.
                    let is_worse = memory_pressure
                        .as_ref()
                        .is_none_or(|(rss, _)| *vm_rss_mb > *rss);
                    if is_worse {
                        memory_pressure = Some((*vm_rss_mb, package_name.clone()));
                    }
                },
                // InterAppInteraction / ActivityLaunch is intentionally not
                // actioned here (the rule was removed in Fix 2). Two reasons
                // compound: the privacy air-gap nulls `source_package` for
                // binder transactions — only a uid survives — so the launch
                // target is unknowable without collector-side uid→package
                // resolution; and PreWarmProcess, its one useful action, is
                // outside the RuleBased capability. The branch could therefore
                // never fire in replay or production, and would only ever yield
                // capability denials. Closing this gap is collector-side work
                // (uid→package), tracked separately.
                //
                // FileActivity is likewise not actioned: its only useful action
                // is PrefetchFile, which the RuleBased capability forbids —
                // speculative file IO belongs to the richer LocalEvaluator /
                // CloudLlm tier (which do allow PrefetchFile). Emitting it here
                // would only produce perpetual capability denials in the audit
                // log.
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
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::KeepAlive,
                    target: Some(from_app),
                    urgency: ActionUrgency::Immediate,
                }],
                rationale_tags: vec!["file_received".into()],
            });
        }

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

        if let Some(target) = observed_foreground_apps.first().cloned() {
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::SwitchToApp(target.clone()),
                confidence: 0.80,
                risk_level: RiskLevel::Low,
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

        // Memory pressure: a heavy / swapped process is trimmed by releasing its
        // non-critical memory. ReleaseMemory is within the RuleBased capability;
        // the target (the offending package) was observed this window, so it
        // passes the policy in-context check. A `None` target means a
        // system-wide trim when the process package is unknown.
        if let Some((_, target)) = memory_pressure {
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::Idle,
                confidence: 0.65,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::ReleaseMemory,
                    target,
                    urgency: ActionUrgency::Immediate,
                }],
                rationale_tags: vec!["memory_pressure".into()],
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
