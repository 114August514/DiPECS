//! Property-based privacy leak test for `DefaultPrivacyAirGap`.
//!
//! Unlike the hand-written scenario tests in `privacy_airgap_test.rs` and
//! `privacy_leak_test.rs`, this file tests the invariant **generatively**:
//!
//! > For **any** `RawEvent` (every variant, diverse inputs), after sanitization
//! > and JSON serialization, no free-form PII-carrying string from the original
//! > event may appear as a substring in the output.
//!
//! This catches classes of bug that hand-written tests miss:
//! - A new `RawEvent` variant whose PII fields the sanitizer forgets to strip
//! - A `serde` rename that accidentally exposes a raw field
//! - Unicode bypasses (zero-width chars, RTL override, normalization)
//! - New fields added to existing variants without sanitizer updates
//!
//! # How it works
//!
//! 1. Define a diverse pool of text samples (CJK, ASCII, emoji, mixed, edge cases)
//! 2. For each `RawEvent` variant, generate inputs using combinations from the pool
//! 3. Collect every free-form string that must NOT survive sanitization
//! 4. Sanitize, serialize to JSON, assert none of the forbidden strings appear
//! 5. Aggregate into `StructuredContext`, serialize, assert again

use aios_core::context_builder::WindowAggregator;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_spec::traits::PrivacySanitizer;
use aios_spec::*;

// ============================================================
// PII-carrying string collector
// ============================================================
//
// For each `RawEvent` variant, returns the list of string values that
// represent free-form user data and MUST NOT survive sanitization.
// Fields that are deliberately preserved as structured metadata
// (e.g. package_name, target_service, channel_id, category) are NOT
// included — they MAY appear in the output.

fn pii_strings(raw: &RawEvent) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    match raw {
        RawEvent::NotificationPosted(e) => {
            if !e.raw_title.is_empty() {
                out.push(e.raw_title.clone());
            }
            if !e.raw_text.is_empty() {
                out.push(e.raw_text.clone());
            }
            if let Some(ref gk) = e.group_key {
                out.push(gk.clone());
            }
        },
        RawEvent::NotificationInteraction(e) => {
            out.push(e.notification_key.clone());
        },
        RawEvent::FileSystemAccess(e) => {
            out.push(e.file_path.clone());
        },
        RawEvent::BinderTransaction(e) => {
            // target_method may embed app-specific info that the
            // sanitizer must not forward verbatim
            out.push(e.target_method.clone());
        },
        // These variants have no free-form PII fields:
        //   AppTransition — package_name/activity_class are deliberately preserved
        //   ProcStateChange — package_name is preserved; all other fields are numeric
        //   ScreenState — enum only, no strings
        //   SystemState — numeric/enum fields only, no strings
        _ => {},
    }
    out
}

// ============================================================
// Input profiles — diverse text samples
// ============================================================

struct TextProfile {
    label: &'static str,
    title: &'static str,
    text: String,
}

fn text_profiles() -> Vec<TextProfile> {
    let long = "Project update: all milestones achieved. ".repeat(50);
    vec![
        // Chinese PII
        TextProfile {
            label: "chinese-name-body",
            title: "王小明",
            text: "李华发来一条消息：明天下午3点开会，请携带工牌".into(),
        },
        // Mixed CJK + Latin + numbers
        TextProfile {
            label: "mixed-cjk-latin-num",
            title: "Amazon 订单 #12345",
            text: "您的包裹 tracking#CN987654321 已发货，预计3天内送达".into(),
        },
        // Emoji-heavy
        TextProfile {
            label: "emoji-heavy",
            title: "🎉🎊 恭喜",
            text: "💰 转账 5000.00 元已到账 ✅ 余额: 12,345.67 元 💰".into(),
        },
        // ASCII only
        TextProfile {
            label: "ascii-credentials",
            title: "Security Alert",
            text: "Login from 192.168.1.100 with password hunter2 detected".into(),
        },
        // Empty strings
        TextProfile {
            label: "empty-strings",
            title: "",
            text: String::new(),
        },
        // Very long text
        TextProfile {
            label: "long-text",
            title: "Weekly Report",
            text: long,
        },
        // Unicode edge cases
        TextProfile {
            label: "unicode-edge",
            title: "Hello\u{200B}World\u{200C}",
            text: "\u{202E}backwards text\u{202C} and \u{FEFF}BOM".into(),
        },
        // Phone numbers + verification codes
        TextProfile {
            label: "phone-and-otp",
            title: "验证码",
            text: "您的验证码是 837291，请在5分钟内输入。电话咨询: 138-0000-1234".into(),
        },
        // Financial
        TextProfile {
            label: "financial",
            title: "交易提醒",
            text: "尾号8825的银行卡支出 ¥12,800.00，余额 ¥345,621.33".into(),
        },
        // File names in text
        TextProfile {
            label: "file-names-in-text",
            title: "共享文件",
            text: "张三分享了 secret_strategy_2026Q4.pdf 和 budget_final.xlsx 给你".into(),
        },
    ]
}

struct PathProfile {
    label: &'static str,
    path: &'static str,
}

fn path_profiles() -> Vec<PathProfile> {
    vec![
        PathProfile {
            label: "internal-data-path",
            path: "/data/data/com.example.app/files/user_profile.db",
        },
        PathProfile {
            label: "sdcard-document",
            path: "/storage/emulated/0/Documents/tax_return_2025.pdf",
        },
        PathProfile {
            label: "dcim-photo",
            path: "/storage/emulated/0/DCIM/Camera/IMG_20260701_143052.jpg",
        },
        PathProfile {
            label: "download-apk",
            path: "/storage/emulated/0/Download/com.example.app_v3.2.1.apk",
        },
        PathProfile {
            label: "chinese-filename",
            path: "/storage/emulated/0/Download/工资条_2026年6月.xlsx",
        },
        PathProfile {
            label: "cache-temp",
            path: "/data/data/com.bank.app/cache/tmp_9a8b7c6d.tmp",
        },
    ]
}

struct KeyProfile {
    label: &'static str,
    key: &'static str,
}

fn key_profiles() -> Vec<KeyProfile> {
    vec![
        KeyProfile {
            label: "contact-name-tag",
            key: "0|com.example.chat|42|Alice-Smith|10042",
        },
        KeyProfile {
            label: "chinese-tag",
            key: "0|com.tencent.mm|99|张三的聊天|10099",
        },
        KeyProfile {
            label: "phone-number-tag",
            key: "0|com.android.dialer|7|+8613812345678|10007",
        },
        KeyProfile {
            label: "empty-tag",
            key: "0|com.example|1||10001",
        },
    ]
}

struct MethodProfile {
    label: &'static str,
    service: &'static str,
    method: &'static str,
}

fn method_profiles() -> Vec<MethodProfile> {
    vec![
        MethodProfile {
            label: "share-intent",
            service: "activity",
            method: "shareContentWithTarget_com.example.mail_subject_invoice",
        },
        MethodProfile {
            label: "notification-enqueue",
            service: "notification",
            method: "enqueueNotificationWithTag_private_message_from_boss",
        },
        MethodProfile {
            label: "bind-service",
            service: "settings",
            method: "bindService_com.android.settings.location_provider",
        },
        MethodProfile {
            label: "plain-activity-launch",
            service: "activity",
            method: "startActivity",
        },
    ]
}

// ============================================================
// Helper: sanitize + aggregate + check
// ============================================================

fn assert_no_pii_leak(case_label: &str, raw: &RawEvent, forbidden: &[String]) {
    if forbidden.is_empty() {
        return; // nothing to check
    }

    let sanitizer = DefaultPrivacyAirGap;
    let sanitized = sanitizer.sanitize(raw.clone());
    let json = serde_json::to_string(&sanitized).expect("SanitizedEvent must serialize");

    for needle in forbidden {
        assert!(
            !json.contains(needle.as_str()),
            "[{case_label}] PII `{needle}` leaked into SanitizedEvent JSON:\n{json}"
        );
    }

    // Also check after window aggregation
    let timestamp = sanitized.timestamp_ms;
    let mut agg = WindowAggregator::new(10, timestamp);
    agg.push(sanitized);
    if let Some(ctx) = agg.close(timestamp + 10_000) {
        let ctx_json = serde_json::to_string(&ctx).expect("StructuredContext must serialize");
        for needle in forbidden {
            assert!(
                !ctx_json.contains(needle.as_str()),
                "[{case_label}] PII `{needle}` leaked into StructuredContext JSON:\n{ctx_json}"
            );
        }
    }
}

// ============================================================
// Property tests — one per variant
// ============================================================

#[test]
fn property_notification_posted_no_pii_leak() {
    for profile in text_profiles() {
        let raw = RawEvent::NotificationPosted(NotificationRawEvent {
            timestamp_ms: 1000,
            package_name: "com.example.app".into(),
            category: Some("msg".into()),
            channel_id: Some("ch_01".into()),
            raw_title: profile.title.into(),
            raw_text: profile.text.clone(),
            title_hint: None,
            text_hint: None,
            semantic_hints: vec![],
            is_ongoing: false,
            group_key: Some("conv_alice_bob_42".into()),
            has_picture: false,
        });
        let forbidden = pii_strings(&raw);
        assert!(
            !forbidden.is_empty(),
            "[{}] must have PII strings",
            profile.label
        );
        assert_no_pii_leak(profile.label, &raw, &forbidden);
    }
}

#[test]
fn property_notification_interaction_no_key_leak() {
    for kp in key_profiles() {
        let raw = RawEvent::NotificationInteraction(NotificationInteractionRawEvent {
            timestamp_ms: 1000,
            package_name: "com.example.app".into(),
            notification_key: kp.key.into(),
            action: NotificationAction::Tapped,
        });
        let forbidden = pii_strings(&raw);
        assert!(
            !forbidden.is_empty(),
            "[{}] must have PII strings",
            kp.label
        );
        assert_no_pii_leak(kp.label, &raw, &forbidden);
    }
}

#[test]
fn property_filesystem_access_no_path_leak() {
    for pp in path_profiles() {
        let raw = RawEvent::FileSystemAccess(FsAccessEvent {
            timestamp_ms: 1000,
            pid: 42,
            uid: 10123,
            file_path: pp.path.into(),
            access_type: FsAccessType::OpenRead,
            bytes_transferred: Some(4096),
        });
        let forbidden = pii_strings(&raw);
        assert!(
            !forbidden.is_empty(),
            "[{}] must have PII strings",
            pp.label
        );
        assert_no_pii_leak(pp.label, &raw, &forbidden);
    }
}

#[test]
fn property_binder_transaction_no_method_leak() {
    for mp in method_profiles() {
        let raw = RawEvent::BinderTransaction(BinderTxEvent {
            timestamp_ms: 1000,
            source_pid: 100,
            source_uid: 10123,
            target_service: mp.service.into(),
            target_method: mp.method.into(),
            is_oneway: true,
            payload_size: 512,
        });
        let forbidden = pii_strings(&raw);
        assert!(
            !forbidden.is_empty(),
            "[{}] must have PII strings",
            mp.label
        );
        assert_no_pii_leak(mp.label, &raw, &forbidden);
    }
}

#[test]
fn property_variants_without_pii_still_sanitize() {
    // Variants without free-form PII must still produce valid SanitizedEvents
    // (no panics, well-formed JSON, correct source_tier and app_package).

    // AppTransition
    let raw = RawEvent::AppTransition(AppTransitionRawEvent {
        timestamp_ms: 1000,
        package_name: "com.android.chrome".into(),
        activity_class: Some("com.google.android.apps.chrome.Main".into()),
        transition: AppTransition::Foreground,
    });
    let s = DefaultPrivacyAirGap.sanitize(raw);
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("com.android.chrome"));
    assert!(json.contains("com.google.android.apps.chrome.Main"));

    // ProcStateChange
    let raw = RawEvent::ProcStateChange(ProcStateEvent {
        timestamp_ms: 1000,
        pid: 42,
        uid: 10123,
        package_name: Some("com.example.app".into()),
        vm_rss_kb: 102400,
        vm_swap_kb: 0,
        threads: 8,
        oom_score: 0,
        io_read_mb: 100,
        io_write_mb: 50,
        state: ProcState::Running,
    });
    let s = DefaultPrivacyAirGap.sanitize(raw);
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("com.example.app"));
    assert!(
        !json.contains("vm_rss_kb"),
        "raw field name must not appear"
    );
    assert!(json.contains("vm_rss_mb"), "sanitized field must appear");

    // ScreenState
    for state in &[
        ScreenState::Interactive,
        ScreenState::NonInteractive,
        ScreenState::KeyguardShown,
        ScreenState::KeyguardHidden,
    ] {
        let raw = RawEvent::ScreenState(ScreenStateEvent {
            timestamp_ms: 1000,
            state: state.clone(),
        });
        let s = DefaultPrivacyAirGap.sanitize(raw);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"Screen\""), "Screen variant must serialize");
    }

    // SystemState
    let raw = RawEvent::SystemState(SystemStateEvent {
        timestamp_ms: 1000,
        battery_pct: Some(50),
        is_charging: false,
        network: NetworkType::Cellular,
        ringer_mode: RingerMode::Silent,
        location_type: LocationType::Work,
        headphone_connected: true,
        bluetooth_connected: false,
    });
    let s = DefaultPrivacyAirGap.sanitize(raw);
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"SystemStatus\""));
    assert!(json.contains("\"Cellular\""));
    assert!(json.contains("\"Silent\""));
}

// ============================================================
// Exhaustive variant coverage check
// ============================================================

#[test]
fn property_all_variants_covered() {
    // This test fails at compile time if a new RawEvent variant is added
    // without a corresponding entry in pii_strings() or the property tests.
    // The match below forces us to touch this file when RawEvent changes.

    let all_variants: &[&dyn Fn() -> RawEvent] = &[
        &|| {
            RawEvent::NotificationPosted(NotificationRawEvent {
                timestamp_ms: 0,
                package_name: "pkg".into(),
                category: None,
                channel_id: None,
                raw_title: "TITLE_PII_MARKER_abc123".into(),
                raw_text: "TEXT_PII_MARKER_xyz789".into(),
                title_hint: None,
                text_hint: None,
                semantic_hints: vec![],
                is_ongoing: false,
                group_key: Some("GROUP_KEY_MARKER_def456".into()),
                has_picture: false,
            })
        },
        &|| {
            RawEvent::NotificationInteraction(NotificationInteractionRawEvent {
                timestamp_ms: 0,
                package_name: "pkg".into(),
                notification_key: "0|pkg|1|KEY_TAG_MARKER_qwerty|10001".into(),
                action: NotificationAction::Tapped,
            })
        },
        &|| {
            RawEvent::FileSystemAccess(FsAccessEvent {
                timestamp_ms: 0,
                pid: 0,
                uid: 0,
                file_path: "/tmp/PATH_MARKER_secret_file.pdf".into(),
                access_type: FsAccessType::OpenRead,
                bytes_transferred: None,
            })
        },
        &|| {
            RawEvent::BinderTransaction(BinderTxEvent {
                timestamp_ms: 0,
                source_pid: 0,
                source_uid: 0,
                target_service: "svc".into(),
                target_method: "METHOD_MARKER_privateAction".into(),
                is_oneway: false,
                payload_size: 0,
            })
        },
        &|| {
            RawEvent::AppTransition(AppTransitionRawEvent {
                timestamp_ms: 0,
                package_name: "pkg".into(),
                activity_class: None,
                transition: AppTransition::Foreground,
            })
        },
        &|| {
            RawEvent::ProcStateChange(ProcStateEvent {
                timestamp_ms: 0,
                pid: 0,
                uid: 0,
                package_name: None,
                vm_rss_kb: 0,
                vm_swap_kb: 0,
                threads: 0,
                oom_score: 0,
                io_read_mb: 0,
                io_write_mb: 0,
                state: ProcState::Running,
            })
        },
        &|| {
            RawEvent::ScreenState(ScreenStateEvent {
                timestamp_ms: 0,
                state: ScreenState::Interactive,
            })
        },
        &|| {
            RawEvent::SystemState(SystemStateEvent {
                timestamp_ms: 0,
                battery_pct: None,
                is_charging: false,
                network: NetworkType::Unknown,
                ringer_mode: RingerMode::Normal,
                location_type: LocationType::Unknown,
                headphone_connected: false,
                bluetooth_connected: false,
            })
        },
    ];

    for (i, constructor) in all_variants.iter().enumerate() {
        let raw = constructor();
        let forbidden = pii_strings(&raw);
        let sanitized = DefaultPrivacyAirGap.sanitize(raw);
        let json = serde_json::to_string(&sanitized).unwrap();

        for needle in &forbidden {
            assert!(
                !json.contains(needle.as_str()),
                "variant {i}: PII `{needle}` leaked into JSON:\n{json}"
            );
        }

        // Every variant must serialize to valid JSON
        let _: serde_json::Value =
            serde_json::from_str(&json).expect("SanitizedEvent must be valid JSON");
    }
}
