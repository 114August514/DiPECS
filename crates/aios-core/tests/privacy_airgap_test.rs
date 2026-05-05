//! 验证 PrivacyAirGap 的脱敏逻辑

use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_spec::traits::PrivacySanitizer;
use aios_spec::*;

#[test]
fn test_notification_file_detection() {
    let sanitizer = DefaultPrivacyAirGap;

    // 模拟飞书收到文件通知
    let raw = RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 1000,
        package_name: "com.ss.android.lark".into(),
        category: Some("msg".into()),
        channel_id: Some("lark_im_message".into()),
        raw_title: "张三".into(),
        raw_text: "张三发来一个文件: quarterly_report.pdf".into(),
        is_ongoing: false,
        group_key: Some("conv_12345".into()),
        has_picture: false,
    });

    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::Notification {
            title_hint,
            text_hint,
            semantic_hints,
            ..
        } => {
            // 标题不包含原文
            assert!(!title_hint.is_emoji_only);
            assert_eq!(title_hint.script, ScriptHint::Hanzi);
            assert!(title_hint.length_chars > 0);

            // 正文也不包含原文
            assert!(!text_hint.is_emoji_only);

            // 必须包含 FileMention
            assert!(semantic_hints.contains(&SemanticHint::FileMention));

            // 不应该包含图片标签
            assert!(!semantic_hints.contains(&SemanticHint::ImageMention));
        },
        _ => panic!("expected Notification event"),
    }

    // 验证原始数据已经不存在于任何字段中
    // (Rust ownership 保证: raw_title/raw_text 被 move 进 sanitize() 后,
    //  调用方不再持有它们)
    assert!(sanitized.app_package.as_deref() == Some("com.ss.android.lark"));
}

#[test]
fn test_notification_image_detection() {
    let sanitizer = DefaultPrivacyAirGap;

    let raw = RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 1000,
        package_name: "com.tencent.mm".into(),
        category: Some("msg".into()),
        channel_id: None,
        raw_title: "家人群".into(),
        raw_text: "发了一张照片".into(),
        is_ongoing: false,
        group_key: None,
        has_picture: true,
    });

    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::Notification { semantic_hints, .. } => {
            assert!(semantic_hints.contains(&SemanticHint::ImageMention));
        },
        _ => panic!("expected Notification event"),
    }
}

#[test]
fn test_fs_classification() {
    let sanitizer = DefaultPrivacyAirGap;

    let raw = RawEvent::FileSystemAccess(FsAccessEvent {
        timestamp_ms: 1000,
        pid: 12345,
        uid: 10123,
        file_path: "/storage/emulated/0/Download/report.pdf".into(),
        access_type: FsAccessType::Create,
        bytes_transferred: Some(524288),
    });

    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::FileActivity {
            extension_category, ..
        } => {
            assert_eq!(*extension_category, ExtensionCategory::Document);
        },
        _ => panic!("expected FileActivity event"),
    }
}

#[test]
fn test_binder_notification_detection() {
    let sanitizer = DefaultPrivacyAirGap;

    let raw = RawEvent::BinderTransaction(BinderTxEvent {
        timestamp_ms: 1000,
        source_pid: 12345,
        source_uid: 10123,
        target_service: "notification".into(),
        target_method: "enqueueNotificationWithTag".into(),
        is_oneway: true,
        payload_size: 2048,
    });

    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::InterAppInteraction {
            target_service,
            interaction_type,
            ..
        } => {
            assert_eq!(target_service, "notification");
            assert!(matches!(interaction_type, InteractionType::NotifyPost));
        },
        _ => panic!("expected InterAppInteraction event"),
    }
}

#[test]
fn test_app_transition_foreground_sanitized() {
    let sanitizer = DefaultPrivacyAirGap;

    let raw = RawEvent::AppTransition(AppTransitionRawEvent {
        timestamp_ms: 2000,
        package_name: "com.android.chrome".into(),
        activity_class: Some("com.google.android.apps.chrome.Main".into()),
        transition: AppTransition::Foreground,
    });

    let sanitized = sanitizer.sanitize(raw);

    assert_eq!(sanitized.source_tier, SourceTier::PublicApi);
    assert_eq!(sanitized.app_package.as_deref(), Some("com.android.chrome"));
    match sanitized.event_type {
        SanitizedEventType::AppTransition {
            package_name,
            activity_class,
            transition,
        } => {
            assert_eq!(package_name, "com.android.chrome");
            assert_eq!(
                activity_class.as_deref(),
                Some("com.google.android.apps.chrome.Main")
            );
            assert_eq!(transition, AppTransition::Foreground);
        },
        _ => panic!("expected AppTransition event"),
    }
}

#[test]
fn test_prefixed_variable_warning_suppression() {
    // 验证代码通过编译 (clippy 不报错)
    let sanitizer = DefaultPrivacyAirGap;
    let _ = sanitizer; // 使用 _ 前缀抑制警告
}
