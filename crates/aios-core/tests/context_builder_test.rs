//! 验证 WindowAggregator 的窗口聚合逻辑

use aios_core::context_builder::WindowAggregator;
use aios_spec::*;

fn make_sanitized_event(
    id: &str,
    ts: i64,
    tier: SourceTier,
    event_type: SanitizedEventType,
) -> SanitizedEvent {
    SanitizedEvent {
        event_id: id.into(),
        timestamp_ms: ts,
        event_type,
        source_tier: tier,
        app_package: None,
        uid: None,
    }
}

fn make_proc_event(pid: u32, pkg: &str) -> SanitizedEvent {
    make_sanitized_event(
        "proc",
        5000,
        SourceTier::Daemon,
        SanitizedEventType::ProcessResource {
            pid,
            package_name: Some(pkg.into()),
            vm_rss_mb: 100,
            vm_swap_mb: 0,
            thread_count: 4,
            oom_score: 0,
        },
    )
}

fn make_notif_event(pkg: &str, hints: Vec<SemanticHint>) -> SanitizedEvent {
    make_sanitized_event(
        "notif",
        5000,
        SourceTier::PublicApi,
        SanitizedEventType::Notification {
            source_package: pkg.into(),
            category: None,
            channel_id: None,
            title_hint: TextHint {
                length_chars: 3,
                script: ScriptHint::Latin,
                is_emoji_only: false,
            },
            text_hint: TextHint {
                length_chars: 10,
                script: ScriptHint::Latin,
                is_emoji_only: false,
            },
            semantic_hints: hints,
            is_ongoing: false,
            group_key: None,
        },
    )
}

fn make_fs_event(ext: ExtensionCategory) -> SanitizedEvent {
    make_sanitized_event(
        "fs",
        5000,
        SourceTier::Daemon,
        SanitizedEventType::FileActivity {
            package_name: None,
            extension_category: ext,
            activity_type: FsActivityType::Read,
            is_hot_file: false,
        },
    )
}

fn make_sys_event(battery_pct: u8) -> SanitizedEvent {
    make_sanitized_event(
        "sys",
        6000,
        SourceTier::PublicApi,
        SanitizedEventType::SystemStatus {
            battery_pct: Some(battery_pct),
            is_charging: true,
            network: NetworkType::Wifi,
            ringer_mode: RingerMode::Normal,
            location_type: LocationType::Home,
            headphone_connected: false,
        },
    )
}

// ===== 基本生命周期 =====

#[test]
fn test_new_window_is_empty() {
    let w = WindowAggregator::new(10, 1000);
    assert!(w.is_empty());
    assert_eq!(w.len(), 0);
}

#[test]
fn test_push_increases_length() {
    let mut w = WindowAggregator::new(10, 1000);
    let evt = make_proc_event(1, "com.test");
    w.push(evt);
    assert_eq!(w.len(), 1);
    assert!(!w.is_empty());
}

// ===== 窗口到期 =====

#[test]
fn test_window_not_expired_before_deadline() {
    let w = WindowAggregator::new(10, 1000);
    assert!(!w.is_expired(5000));
}

#[test]
fn test_window_expired_after_deadline() {
    let w = WindowAggregator::new(10, 1000);
    assert!(w.is_expired(11000));
}

#[test]
fn test_window_expired_at_exact_deadline() {
    let w = WindowAggregator::new(10, 1000);
    assert!(w.is_expired(11000));
}

// ===== 窗口关闭 =====

#[test]
fn test_close_empty_window_returns_none() {
    let mut w = WindowAggregator::new(10, 1000);
    let result = w.close(11000);
    assert!(result.is_none());
}

#[test]
fn test_close_non_empty_returns_context() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_proc_event(1, "com.test"));
    let result = w.close(11000);

    assert!(result.is_some());
    let ctx = result.unwrap();
    assert_eq!(ctx.window_start_ms, 1000);
    assert_eq!(ctx.window_end_ms, 11000);
    assert_eq!(ctx.duration_secs, 10);
    assert_eq!(ctx.events.len(), 1);
    assert!(!ctx.window_id.is_empty());
}

#[test]
fn test_close_resets_buffer() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_proc_event(1, "com.test"));
    let _ = w.close(11000);
    assert!(w.is_empty());
    assert_eq!(w.len(), 0);
}

#[test]
fn test_close_updates_window_start() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_proc_event(1, "com.test"));
    let _ = w.close(11000);
    // 新窗口从11000开始, 12000不应过期
    assert!(!w.is_expired(12000));
}

// ===== build_summary 聚合 =====

#[test]
fn test_summary_foreground_apps() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_proc_event(1, "com.a"));
    w.push(make_proc_event(2, "com.b"));
    // 添加 InterAppInteraction
    w.push(make_sanitized_event(
        "ia",
        5000,
        SourceTier::Daemon,
        SanitizedEventType::InterAppInteraction {
            source_package: Some("com.c".into()),
            target_service: "activity".into(),
            interaction_type: InteractionType::ActivityLaunch,
        },
    ));
    let ctx = w.close(11000).unwrap();
    let fg = &ctx.summary.foreground_apps;
    assert!(fg.contains(&"com.a".to_string()));
    assert!(fg.contains(&"com.b".to_string()));
    assert!(fg.contains(&"com.c".to_string()));
    // 去重
    let unique: std::collections::HashSet<_> = fg.iter().collect();
    assert_eq!(fg.len(), unique.len());
}

#[test]
fn test_summary_foreground_apps_from_app_transition() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_sanitized_event(
        "fg",
        5000,
        SourceTier::PublicApi,
        SanitizedEventType::AppTransition {
            package_name: "com.android.chrome".into(),
            activity_class: Some("Main".into()),
            transition: AppTransition::Foreground,
        },
    ));
    w.push(make_sanitized_event(
        "bg",
        6000,
        SourceTier::PublicApi,
        SanitizedEventType::AppTransition {
            package_name: "com.example.background".into(),
            activity_class: None,
            transition: AppTransition::Background,
        },
    ));

    let ctx = w.close(11000).unwrap();

    assert_eq!(ctx.summary.source_tier, SourceTier::PublicApi);
    assert!(ctx
        .summary
        .foreground_apps
        .contains(&"com.android.chrome".to_string()));
    assert!(!ctx
        .summary
        .foreground_apps
        .contains(&"com.example.background".to_string()));
}

#[test]
fn test_summary_notified_apps() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_notif_event(
        "com.lark",
        vec![SemanticHint::FileMention],
    ));
    w.push(make_notif_event(
        "com.lark",
        vec![SemanticHint::ImageMention],
    ));
    w.push(make_notif_event("com.wechat", vec![]));
    let ctx = w.close(11000).unwrap();
    let notified = &ctx.summary.notified_apps;
    assert!(notified.contains(&"com.lark".to_string()));
    assert!(notified.contains(&"com.wechat".to_string()));
    // 去重
    assert_eq!(notified.len(), 2);
}

#[test]
fn test_summary_semantic_hints() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_notif_event(
        "com.a",
        vec![SemanticHint::FileMention, SemanticHint::LinkAttachment],
    ));
    w.push(make_notif_event(
        "com.b",
        vec![SemanticHint::FileMention, SemanticHint::FinancialContext],
    ));
    let ctx = w.close(11000).unwrap();
    let hints = &ctx.summary.all_semantic_hints;
    assert!(hints.contains(&SemanticHint::FileMention));
    assert!(hints.contains(&SemanticHint::LinkAttachment));
    assert!(hints.contains(&SemanticHint::FinancialContext));
    assert_eq!(hints.len(), 3); // FileMention 去重
}

#[test]
fn test_summary_file_activity_counts() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_fs_event(ExtensionCategory::Document));
    w.push(make_fs_event(ExtensionCategory::Document));
    w.push(make_fs_event(ExtensionCategory::Image));
    let ctx = w.close(11000).unwrap();
    let file_act: std::collections::HashMap<_, _> = ctx.summary.file_activity.into_iter().collect();
    assert_eq!(file_act.get(&ExtensionCategory::Document), Some(&2u32));
    assert_eq!(file_act.get(&ExtensionCategory::Image), Some(&1u32));
}

#[test]
fn test_summary_latest_system_status() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_sys_event(80));
    w.push(make_sys_event(50));
    let ctx = w.close(11000).unwrap();
    let status = ctx.summary.latest_system_status.unwrap();
    assert_eq!(status.battery_pct, Some(50)); // latest wins
    assert!(status.is_charging);
}

#[test]
fn test_summary_source_tier_daemon_wins() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_notif_event("com.a", vec![])); // PublicApi
    w.push(make_proc_event(1, "com.b")); // Daemon
    let ctx = w.close(11000).unwrap();
    assert_eq!(ctx.summary.source_tier, SourceTier::Daemon);
}

#[test]
fn test_summary_source_tier_public_api_only() {
    let mut w = WindowAggregator::new(10, 1000);
    w.push(make_notif_event("com.a", vec![])); // PublicApi
    let ctx = w.close(11000).unwrap();
    assert_eq!(ctx.summary.source_tier, SourceTier::PublicApi);
}

// ===== 多窗口循环 =====

#[test]
fn test_multiple_windows_cycle() {
    let mut w = WindowAggregator::new(5, 0);
    w.push(make_proc_event(1, "com.first"));
    let ctx1 = w.close(5000).unwrap();
    assert_eq!(ctx1.events.len(), 1);

    w.push(make_proc_event(2, "com.second"));
    w.push(make_proc_event(3, "com.third"));
    let ctx2 = w.close(10000).unwrap();
    assert_eq!(ctx2.events.len(), 2);
    assert_eq!(ctx2.window_start_ms, 5000);
    assert_eq!(ctx2.window_end_ms, 10000);
}
