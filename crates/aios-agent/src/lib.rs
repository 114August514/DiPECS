//! # aios-agent — Cloud LLM 代理
//!
//! 职责: 将 StructuredContext 发送给云端 LLM, 接收 IntentBatch 返回。
//!
//! 当前阶段提供 MockCloudProxy, 用于打通端到端链路。
//! 后续替换为真实的 HTTPS 通信 (reqwest + rustls)。

use aios_spec::{
    ActionType, ActionUrgency, AppTransition, Intent, IntentBatch, IntentType, RiskLevel,
    SanitizedEventType, SemanticHint, StructuredContext, SuggestedAction,
};
use uuid::Uuid;

/// Mock CloudProxy — 用于开发阶段打通 daemon 全链路。
///
/// 根据 StructuredContext 中的事件类型生成模拟意图:
/// - 通知含 FileMention → 建议 OpenApp("files")
/// - 文件活动 → 建议 HandleFile
/// - ActivityLaunch → 建议 PreWarmProcess
/// - 屏幕亮起 → 建议 KeepAlive
/// - 低电量 → 建议 ReleaseMemory
/// - 无特殊事件 → Idle
pub struct MockCloudProxy;

impl MockCloudProxy {
    /// 评估一个上下文窗口, 返回模拟的 IntentBatch。
    ///
    /// 此方法为同步调用, 不引入 async。后续真实 CloudProxy
    /// 将包含超时降级和熔断器。
    pub fn evaluate(context: &StructuredContext) -> IntentBatch {
        let intents = Self::generate_intents(context);

        IntentBatch {
            window_id: context.window_id.clone(),
            intents,
            generated_at_ms: context.window_end_ms,
            model: "mock-cloud-proxy-v0.1".to_string(),
        }
    }

    fn generate_intents(context: &StructuredContext) -> Vec<Intent> {
        let mut intents = Vec::new();
        let summary = &context.summary;

        // 从窗口事件中提取信号
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
                    extension_category, ..
                } => {
                    // 文件活动单独生成 HandleFile 意图
                    intents.push(Intent {
                        intent_id: new_id(),
                        intent_type: IntentType::HandleFile(extension_category.clone()),
                        confidence: 0.75,
                        risk_level: RiskLevel::Low,
                        suggested_actions: vec![SuggestedAction {
                            action_type: ActionType::PrefetchFile,
                            target: None,
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

        // 生成意图
        if has_file_mention {
            // 检测到文件相关通知 → 用户可能即将打开文件管理器
            let from_app = notified_apps.first().cloned().unwrap_or_default();
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::OpenApp(from_app.clone()),
                confidence: 0.70,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::PreWarmProcess,
                    target: Some(from_app),
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

        // 始终包含一个 Idle 兜底, 确认 daemon 链路上报正常
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
            "MockCloudProxy generated intents"
        );

        intents
    }
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}
