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
        title_hint: None,
        text_hint: None,
        semantic_hints: vec![],
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
        title_hint: None,
        text_hint: None,
        semantic_hints: vec![],
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
fn test_notification_uses_collector_safe_hints_when_raw_text_is_redacted() {
    let sanitizer = DefaultPrivacyAirGap;

    let raw = RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 1000,
        package_name: "com.ss.android.lark".into(),
        category: Some("msg".into()),
        channel_id: Some("lark_im_message".into()),
        raw_title: "".into(),
        raw_text: "".into(),
        title_hint: Some(TextHint {
            length_chars: 2,
            script: ScriptHint::Hanzi,
            is_emoji_only: false,
        }),
        text_hint: Some(TextHint {
            length_chars: 20,
            script: ScriptHint::Mixed,
            is_emoji_only: false,
        }),
        semantic_hints: vec![SemanticHint::FileMention],
        is_ongoing: false,
        group_key: None,
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
            assert_eq!(title_hint.length_chars, 2);
            assert_eq!(title_hint.script, ScriptHint::Hanzi);
            assert_eq!(text_hint.length_chars, 20);
            assert_eq!(text_hint.script, ScriptHint::Mixed);
            assert!(semantic_hints.contains(&SemanticHint::FileMention));
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

// ===== PII 回归测试 =====

#[test]
fn test_chinese_names_not_leaked() {
    let sanitizer = DefaultPrivacyAirGap;
    let raw = RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 1000,
        package_name: "com.tencent.mm".into(),
        category: Some("msg".into()),
        channel_id: None,
        raw_title: "王小明".into(),
        raw_text: "李华发来一条消息：明天开会".into(),
        title_hint: None,
        text_hint: None,
        semantic_hints: vec![],
        is_ongoing: false,
        group_key: None,
        has_picture: false,
    });
    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::Notification {
            title_hint,
            text_hint,
            ..
        } => {
            // 元数据保留，原文不保留
            assert!(title_hint.length_chars > 0);
            assert_eq!(title_hint.script, ScriptHint::Hanzi);
            assert!(text_hint.length_chars > 0);
            assert_eq!(text_hint.script, ScriptHint::Hanzi);
            // 原始姓名无法通过 SanitizedEvent 的任何字段访问
        },
        _ => panic!("expected Notification event"),
    }
}

#[test]
fn test_phone_numbers_not_leaked() {
    let sanitizer = DefaultPrivacyAirGap;
    let raw = RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 1000,
        package_name: "com.android.dialer".into(),
        category: Some("call".into()),
        channel_id: None,
        raw_title: "未接来电".into(),
        raw_text: "13812345678 来电".into(),
        title_hint: None,
        text_hint: None,
        semantic_hints: vec![],
        is_ongoing: false,
        group_key: None,
        has_picture: false,
    });
    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::Notification { text_hint, .. } => {
            // 14 个字符的原文长度保留，但原文内容不保留
            assert_eq!(text_hint.length_chars, 14);
            // 无 FileMention, ImageMention 等语义标记（纯电话号码）
        },
        _ => panic!("expected Notification event"),
    }
}

#[test]
fn test_file_path_stripped_only_category_survives() {
    let sanitizer = DefaultPrivacyAirGap;
    let raw = RawEvent::FileSystemAccess(FsAccessEvent {
        timestamp_ms: 1000,
        pid: 12345,
        uid: 10123,
        file_path: "/data/data/com.example/files/user_profile.db".into(),
        access_type: FsAccessType::OpenRead,
        bytes_transferred: Some(4096),
    });
    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::FileActivity {
            extension_category, ..
        } => {
            // .db 映射到 Other
            assert_eq!(*extension_category, ExtensionCategory::Other);
            // 完整路径 /data/data/com.example/files/user_profile.db 已被丢弃
        },
        _ => panic!("expected FileActivity event"),
    }
}

#[test]
fn test_binder_payload_not_preserved() {
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
        SanitizedEventType::InterAppInteraction { .. } => {
            // payload_size 不出现于 SanitizedEvent 的任何字段 —
            // 这是编译期保证（InterAppInteraction 变体无此字段）
        },
        _ => panic!("expected InterAppInteraction event"),
    }
    // Binder 事件通常无 app_package
    assert!(sanitized.app_package.is_none());
}

#[test]
fn test_emoji_only_notification_text() {
    let sanitizer = DefaultPrivacyAirGap;
    let raw = RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 1000,
        package_name: "com.whatsapp".into(),
        category: Some("msg".into()),
        channel_id: None,
        raw_title: "👍".into(),
        raw_text: "😂🎉👍".into(),
        title_hint: None,
        text_hint: None,
        semantic_hints: vec![],
        is_ongoing: false,
        group_key: None,
        has_picture: false,
    });
    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::Notification {
            title_hint,
            text_hint,
            ..
        } => {
            assert!(title_hint.is_emoji_only);
            assert_eq!(title_hint.length_chars, 1);
            assert!(text_hint.is_emoji_only);
            assert_eq!(text_hint.length_chars, 3);
            assert_eq!(text_hint.script, ScriptHint::Unknown);
        },
        _ => panic!("expected Notification event"),
    }
}

#[test]
fn test_mixed_chinese_english_notification_text() {
    let sanitizer = DefaultPrivacyAirGap;
    let raw = RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 1000,
        package_name: "com.example.app".into(),
        category: Some("msg".into()),
        channel_id: None,
        raw_title: "Hello 世界".into(),
        raw_text: "你的order已发货 tracking#12345".into(),
        title_hint: None,
        text_hint: None,
        semantic_hints: vec![],
        is_ongoing: false,
        group_key: None,
        has_picture: false,
    });
    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::Notification {
            title_hint,
            text_hint,
            ..
        } => {
            assert_eq!(title_hint.script, ScriptHint::Mixed);
            assert_eq!(text_hint.script, ScriptHint::Mixed);
            assert!(!title_hint.is_emoji_only);
            assert!(!text_hint.is_emoji_only);
        },
        _ => panic!("expected Notification event"),
    }
}

#[test]
fn test_empty_notification_text() {
    let sanitizer = DefaultPrivacyAirGap;
    let raw = RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 1000,
        package_name: "com.example.app".into(),
        category: Some("msg".into()),
        channel_id: None,
        raw_title: "".into(),
        raw_text: "".into(),
        title_hint: None,
        text_hint: None,
        semantic_hints: vec![],
        is_ongoing: false,
        group_key: None,
        has_picture: false,
    });
    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::Notification {
            title_hint,
            text_hint,
            ..
        } => {
            assert_eq!(title_hint.length_chars, 0);
            assert_eq!(title_hint.script, ScriptHint::Unknown);
            assert!(!title_hint.is_emoji_only);
            assert_eq!(text_hint.length_chars, 0);
            assert_eq!(text_hint.script, ScriptHint::Unknown);
            assert!(!text_hint.is_emoji_only);
        },
        _ => panic!("expected Notification event"),
    }
}

#[test]
fn test_very_long_notification_text_length_preserved_content_not_leaked() {
    let sanitizer = DefaultPrivacyAirGap;
    let long_text = "a".repeat(10000);
    let expected_len = long_text.chars().count();
    let raw = RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 1000,
        package_name: "com.example.app".into(),
        category: Some("msg".into()),
        channel_id: None,
        raw_title: "Long message".into(),
        raw_text: long_text,
        title_hint: None,
        text_hint: None,
        semantic_hints: vec![],
        is_ongoing: false,
        group_key: None,
        has_picture: false,
    });
    let sanitized = sanitizer.sanitize(raw);

    match &sanitized.event_type {
        SanitizedEventType::Notification { text_hint, .. } => {
            assert_eq!(text_hint.length_chars, expected_len);
            assert_eq!(text_hint.script, ScriptHint::Latin);
            assert!(!text_hint.is_emoji_only);
            // 10000 个 'a' — 原文内容不可访问，仅长度元数据保留
        },
        _ => panic!("expected Notification event"),
    }
}

#[test]
fn test_sanitize_with_tier_overrides_default() {
    let sanitizer = DefaultPrivacyAirGap;

    // ProcStateChange defaults to SourceTier::Daemon; override to PublicApi.
    let raw = RawEvent::ProcStateChange(ProcStateEvent {
        timestamp_ms: 1000,
        pid: 42,
        uid: 10123,
        package_name: Some("com.test".into()),
        vm_rss_kb: 1024,
        vm_swap_kb: 0,
        threads: 4,
        oom_score: 0,
        io_read_mb: 0,
        io_write_mb: 0,
        state: ProcState::Running,
    });

    let sanitized = sanitizer.sanitize_with_tier(raw, SourceTier::PublicApi);
    assert_eq!(sanitized.source_tier, SourceTier::PublicApi);
}

#[test]
fn test_sanitize_with_tier_overrides_public_api_to_daemon() {
    let sanitizer = DefaultPrivacyAirGap;

    // AppTransition defaults to SourceTier::PublicApi; override to Daemon.
    let raw = RawEvent::AppTransition(AppTransitionRawEvent {
        timestamp_ms: 1000,
        package_name: "com.test".into(),
        activity_class: None,
        transition: AppTransition::Foreground,
    });

    let sanitized = sanitizer.sanitize_with_tier(raw, SourceTier::Daemon);
    assert_eq!(sanitized.source_tier, SourceTier::Daemon);
}

// ===== 之前缺失的 variant 测试 =====

#[test]
fn test_screen_state_sanitized() {
    let sanitizer = DefaultPrivacyAirGap;
    let raw = RawEvent::ScreenState(ScreenStateEvent {
        timestamp_ms: 1000,
        state: ScreenState::Interactive,
    });
    let sanitized = sanitizer.sanitize(raw);

    assert_eq!(sanitized.source_tier, SourceTier::PublicApi);
    assert!(sanitized.app_package.is_none());
    match sanitized.event_type {
        SanitizedEventType::Screen { state } => {
            assert_eq!(state, ScreenState::Interactive);
        },
        _ => panic!("expected Screen event"),
    }
}

#[test]
fn test_system_state_sanitized() {
    let sanitizer = DefaultPrivacyAirGap;
    let raw = RawEvent::SystemState(SystemStateEvent {
        timestamp_ms: 2000,
        battery_pct: Some(85),
        is_charging: true,
        network: NetworkType::Wifi,
        ringer_mode: RingerMode::Vibrate,
        location_type: LocationType::Unknown,
        headphone_connected: false,
        bluetooth_connected: true,
    });
    let sanitized = sanitizer.sanitize(raw);

    assert_eq!(sanitized.source_tier, SourceTier::PublicApi);
    assert!(sanitized.app_package.is_none());
    match sanitized.event_type {
        SanitizedEventType::SystemStatus {
            battery_pct,
            is_charging,
            network,
            ringer_mode,
            ..
        } => {
            assert_eq!(battery_pct, Some(85));
            assert!(is_charging);
            assert_eq!(network, NetworkType::Wifi);
            assert_eq!(ringer_mode, RingerMode::Vibrate);
        },
        _ => panic!("expected SystemStatus event"),
    }
}

#[test]
fn test_notification_interaction_drops_key_and_preserves_package() {
    let sanitizer = DefaultPrivacyAirGap;
    let raw = RawEvent::NotificationInteraction(NotificationInteractionRawEvent {
        timestamp_ms: 1000,
        package_name: "com.example.app".into(),
        notification_key: "0|com.example|42|alice-thread|10042".into(),
        action: NotificationAction::Tapped,
    });
    let sanitized = sanitizer.sanitize(raw);

    assert_eq!(sanitized.source_tier, SourceTier::PublicApi);
    assert_eq!(sanitized.app_package.as_deref(), Some("com.example.app"));
    match sanitized.event_type {
        SanitizedEventType::Notification {
            source_package,
            group_key,
            title_hint,
            text_hint,
            semantic_hints,
            ..
        } => {
            assert_eq!(source_package, "com.example.app");
            // notification_key 必须被丢弃
            assert!(group_key.is_none());
            // 交互事件无标题/正文
            assert_eq!(title_hint.length_chars, 0);
            assert_eq!(text_hint.length_chars, 0);
            assert!(semantic_hints.is_empty());
        },
        _ => panic!("expected Notification event"),
    }
}
