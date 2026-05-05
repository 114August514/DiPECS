//! 验证 MockCloudProxy 的意图生成逻辑

use aios_agent::MockCloudProxy;
use aios_spec::*;

/// 构建最小 StructuredContext 的辅助函数
fn make_context(events: Vec<SanitizedEvent>, summary: ContextSummary) -> StructuredContext {
    StructuredContext {
        window_id: "test-window-1".into(),
        window_start_ms: 1000,
        window_end_ms: 11000,
        duration_secs: 10,
        events,
        summary,
    }
}

fn make_summary() -> ContextSummary {
    ContextSummary {
        foreground_apps: vec![],
        notified_apps: vec![],
        all_semantic_hints: vec![],
        file_activity: vec![],
        latest_system_status: None,
        source_tier: SourceTier::PublicApi,
    }
}

fn make_notification_event(pkg: &str, hints: Vec<SemanticHint>) -> SanitizedEvent {
    SanitizedEvent {
        event_id: "evt-1".into(),
        timestamp_ms: 5000,
        event_type: SanitizedEventType::Notification {
            source_package: pkg.into(),
            category: Some("msg".into()),
            channel_id: None,
            title_hint: TextHint {
                length_chars: 5,
                script: ScriptHint::Latin,
                is_emoji_only: false,
            },
            text_hint: TextHint {
                length_chars: 20,
                script: ScriptHint::Latin,
                is_emoji_only: false,
            },
            semantic_hints: hints,
            is_ongoing: false,
            group_key: None,
        },
        source_tier: SourceTier::PublicApi,
        app_package: Some(pkg.into()),
        uid: None,
    }
}

fn make_file_activity(ext: ExtensionCategory) -> SanitizedEvent {
    SanitizedEvent {
        event_id: "evt-file".into(),
        timestamp_ms: 5000,
        event_type: SanitizedEventType::FileActivity {
            package_name: Some("com.example.files".into()),
            extension_category: ext,
            activity_type: FsActivityType::Read,
            is_hot_file: false,
        },
        source_tier: SourceTier::Daemon,
        app_package: Some("com.example.files".into()),
        uid: Some(10123),
    }
}

fn make_screen_event(state: ScreenState) -> SanitizedEvent {
    SanitizedEvent {
        event_id: "evt-scr".into(),
        timestamp_ms: 5000,
        event_type: SanitizedEventType::Screen { state },
        source_tier: SourceTier::PublicApi,
        app_package: None,
        uid: None,
    }
}

fn make_system_status(battery_pct: Option<u8>) -> SanitizedEvent {
    SanitizedEvent {
        event_id: "evt-sys".into(),
        timestamp_ms: 5000,
        event_type: SanitizedEventType::SystemStatus {
            battery_pct,
            is_charging: false,
            network: NetworkType::Wifi,
            ringer_mode: RingerMode::Normal,
            location_type: LocationType::Unknown,
            headphone_connected: false,
        },
        source_tier: SourceTier::PublicApi,
        app_package: None,
        uid: None,
    }
}

// ===== 空窗口测试 =====

#[test]
fn test_empty_window_returns_idle() {
    let ctx = make_context(vec![], make_summary());
    let batch = MockCloudProxy::evaluate(&ctx);

    assert_eq!(batch.model, "mock-cloud-proxy-v0.1");
    assert_eq!(batch.intents.len(), 1);
    let intent = &batch.intents[0];
    assert!(matches!(intent.intent_type, IntentType::Idle));
    assert_eq!(intent.suggested_actions.len(), 1);
    assert!(matches!(
        intent.suggested_actions[0].action_type,
        ActionType::NoOp
    ));
}

// ===== FileMention 检测 =====

#[test]
fn test_file_mention_triggers_open_app() {
    let mut summary = make_summary();
    summary.notified_apps = vec!["com.lark".into()];
    let events = vec![make_notification_event(
        "com.lark",
        vec![SemanticHint::FileMention],
    )];
    let ctx = make_context(events, summary);

    let batch = MockCloudProxy::evaluate(&ctx);
    // 空窗口兜底 + file_mention = 2 intents
    let open_app = batch
        .intents
        .iter()
        .find(|i| matches!(i.intent_type, IntentType::OpenApp(_)))
        .expect("should have OpenApp intent");
    assert!(open_app.confidence >= 0.70);
    assert_eq!(open_app.suggested_actions.len(), 1);
    assert!(matches!(
        open_app.suggested_actions[0].action_type,
        ActionType::PreWarmProcess
    ));
}

// ===== ActivityLaunch 检测 =====

#[test]
fn test_activity_launch_triggers_switch_to_app() {
    let mut summary = make_summary();
    summary.foreground_apps = vec!["com.android.chrome".into()];
    let events = vec![SanitizedEvent {
        event_id: "evt-launch".into(),
        timestamp_ms: 5000,
        event_type: SanitizedEventType::InterAppInteraction {
            source_package: Some("com.android.chrome".into()),
            target_service: "activity".into(),
            interaction_type: InteractionType::ActivityLaunch,
        },
        source_tier: SourceTier::Daemon,
        app_package: Some("com.android.chrome".into()),
        uid: Some(10086),
    }];
    let ctx = make_context(events, summary);

    let batch = MockCloudProxy::evaluate(&ctx);
    let switch = batch
        .intents
        .iter()
        .find(|i| matches!(i.intent_type, IntentType::SwitchToApp(_)))
        .expect("should have SwitchToApp intent");
    assert!(switch.confidence >= 0.80);
    assert_eq!(switch.suggested_actions.len(), 2);
    assert!(switch
        .suggested_actions
        .iter()
        .any(|a| matches!(a.action_type, ActionType::PreWarmProcess)));
    assert!(switch
        .suggested_actions
        .iter()
        .any(|a| matches!(a.action_type, ActionType::KeepAlive)));
}

#[test]
fn test_app_transition_foreground_triggers_switch_to_app() {
    let mut summary = make_summary();
    summary.foreground_apps = vec!["com.android.chrome".into()];
    let events = vec![SanitizedEvent {
        event_id: "evt-app-fg".into(),
        timestamp_ms: 5000,
        event_type: SanitizedEventType::AppTransition {
            package_name: "com.android.chrome".into(),
            activity_class: Some("Main".into()),
            transition: AppTransition::Foreground,
        },
        source_tier: SourceTier::PublicApi,
        app_package: Some("com.android.chrome".into()),
        uid: None,
    }];
    let ctx = make_context(events, summary);

    let batch = MockCloudProxy::evaluate(&ctx);

    let switch = batch
        .intents
        .iter()
        .find(|i| matches!(i.intent_type, IntentType::SwitchToApp(_)))
        .expect("should have SwitchToApp intent");
    assert_eq!(
        switch.rationale_tags,
        vec!["app_foreground_observed".to_string()]
    );
}

// ===== FileActivity 处理 =====

#[test]
fn test_file_activity_generates_handle_file() {
    let events = vec![make_file_activity(ExtensionCategory::Document)];
    let ctx = make_context(events, make_summary());

    let batch = MockCloudProxy::evaluate(&ctx);
    let handle = batch
        .intents
        .iter()
        .find(|i| {
            matches!(
                i.intent_type,
                IntentType::HandleFile(ExtensionCategory::Document)
            )
        })
        .expect("should have HandleFile intent");
    assert_eq!(handle.confidence, 0.75);
    assert_eq!(handle.suggested_actions.len(), 1);
    assert!(matches!(
        handle.suggested_actions[0].action_type,
        ActionType::PrefetchFile
    ));
}

#[test]
fn test_multiple_file_activities_generate_multiple_intents() {
    let events = vec![
        make_file_activity(ExtensionCategory::Document),
        make_file_activity(ExtensionCategory::Image),
        make_file_activity(ExtensionCategory::Video),
    ];
    let ctx = make_context(events, make_summary());

    let batch = MockCloudProxy::evaluate(&ctx);
    let handle_count = batch
        .intents
        .iter()
        .filter(|i| matches!(i.intent_type, IntentType::HandleFile(_)))
        .count();
    assert_eq!(handle_count, 3);
}

// ===== 屏幕亮起检测 =====

#[test]
fn test_screen_on_triggers_keepalive() {
    let mut summary = make_summary();
    summary.foreground_apps = vec!["com.example.ui".into()];
    let events = vec![make_screen_event(ScreenState::Interactive)];
    let ctx = make_context(events, summary);

    let batch = MockCloudProxy::evaluate(&ctx);
    let screen_intent = batch
        .intents
        .iter()
        .find(|i| i.rationale_tags.contains(&"screen_on".to_string()))
        .expect("should have screen_on intent");
    assert!(matches!(screen_intent.intent_type, IntentType::Idle));
    assert_eq!(screen_intent.confidence, 0.60);
    assert_eq!(screen_intent.suggested_actions.len(), 1);
    assert!(matches!(
        screen_intent.suggested_actions[0].action_type,
        ActionType::KeepAlive
    ));
}

// ===== 低电量检测 =====

#[test]
fn test_low_battery_triggers_release_memory() {
    let events = vec![make_system_status(Some(15))];
    let ctx = make_context(events, make_summary());

    let batch = MockCloudProxy::evaluate(&ctx);
    let battery_intent = batch
        .intents
        .iter()
        .find(|i| i.rationale_tags.contains(&"low_battery".to_string()))
        .expect("should have low_battery intent");
    assert!(matches!(battery_intent.intent_type, IntentType::Idle));
    assert_eq!(battery_intent.confidence, 0.80);
    assert_eq!(battery_intent.suggested_actions.len(), 1);
    assert!(matches!(
        battery_intent.suggested_actions[0].action_type,
        ActionType::ReleaseMemory
    ));
}

#[test]
fn test_normal_battery_no_release() {
    let events = vec![make_system_status(Some(85))];
    let ctx = make_context(events, make_summary());

    let batch = MockCloudProxy::evaluate(&ctx);
    let has_release = batch
        .intents
        .iter()
        .any(|i| i.rationale_tags.contains(&"low_battery".to_string()));
    assert!(
        !has_release,
        "should not trigger ReleaseMemory at 85% battery"
    );
}

// ===== 组合信号 =====

#[test]
fn test_combined_signals_all_detected() {
    let mut summary = make_summary();
    summary.notified_apps = vec!["com.lark".into()];
    summary.foreground_apps = vec!["com.android.chrome".into()];

    let events = vec![
        make_notification_event("com.lark", vec![SemanticHint::FileMention]),
        SanitizedEvent {
            event_id: "evt-launch".into(),
            timestamp_ms: 5500,
            event_type: SanitizedEventType::InterAppInteraction {
                source_package: Some("com.android.chrome".into()),
                target_service: "activity".into(),
                interaction_type: InteractionType::ActivityLaunch,
            },
            source_tier: SourceTier::Daemon,
            app_package: Some("com.android.chrome".into()),
            uid: Some(10086),
        },
        make_screen_event(ScreenState::Interactive),
        make_file_activity(ExtensionCategory::Image),
        make_system_status(Some(10)),
    ];
    let ctx = make_context(events, summary);

    let batch = MockCloudProxy::evaluate(&ctx);
    let tags: Vec<&str> = batch
        .intents
        .iter()
        .flat_map(|i| i.rationale_tags.iter().map(|s| s.as_str()))
        .collect();

    assert!(
        tags.contains(&"file_received"),
        "should detect file mention"
    );
    assert!(
        tags.contains(&"app_launch_detected"),
        "should detect activity launch"
    );
    assert!(tags.contains(&"screen_on"), "should detect screen on");
    assert!(tags.contains(&"low_battery"), "should detect low battery");
    // FileActivity = Document + Image → 2 HandleFile intents
    let handle_count = batch
        .intents
        .iter()
        .filter(|i| matches!(i.intent_type, IntentType::HandleFile(_)))
        .count();
    assert_eq!(handle_count, 1, "only 1 file_activity event");
}
