use aios_adapter::collection_stats::{RawEventKind, RawEventStats};
use aios_spec::{AppTransition, AppTransitionRawEvent, NotificationRawEvent, RawEvent};

#[test]
fn test_raw_event_stats_counts_app_transitions() {
    let mut stats = RawEventStats::default();
    stats.record(&RawEvent::AppTransition(AppTransitionRawEvent {
        timestamp_ms: 1,
        package_name: "com.android.chrome".into(),
        activity_class: None,
        transition: AppTransition::Foreground,
    }));

    assert_eq!(stats.count(RawEventKind::AppTransition), 1);
    assert_eq!(stats.total(), 1);
    assert!(stats.summary_fields().contains(&("app_transition", 1)));
}

#[test]
fn test_raw_event_stats_summary_line_only_includes_nonzero_counts() {
    let mut stats = RawEventStats::default();
    stats.record(&RawEvent::AppTransition(AppTransitionRawEvent {
        timestamp_ms: 1,
        package_name: "com.android.chrome".into(),
        activity_class: None,
        transition: AppTransition::Foreground,
    }));
    stats.record(&RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 2,
        package_name: "com.ss.android.lark".into(),
        category: Some("msg".into()),
        channel_id: None,
        raw_title: "张三".into(),
        raw_text: "文件".into(),
        is_ongoing: false,
        group_key: None,
        has_picture: false,
    }));

    assert_eq!(
        stats.summary_line(),
        "app_transition=1 notification_posted=1"
    );
}
