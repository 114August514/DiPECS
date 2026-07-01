//! Privacy-leak regressions for `DefaultPrivacyAirGap`.
//!
//! Existing tests in `privacy_airgap_test.rs` check that the *shape* of
//! `SanitizedEvent` is correct (TextHint scripts, ExtensionCategory mapping,
//! semantic hints). They do **not** prove that free-form PII never survives
//! into the JSON-serialized output — and that is the property the teacher's
//! "raw leakage = 0" criterion actually pins.
//!
//! These tests do that the obvious way: build a `RawEvent` with a known
//! distinctive PII fragment, sanitize it (and additionally route it through
//! [`WindowAggregator`] into a [`StructuredContext`]), then serialize to JSON
//! and assert the fragment does not appear as a substring anywhere.
//!
//! The forbidden substrings are picked so they cannot collide with any field
//! name, enum variant, or hint value emitted by the current sanitizer.

use aios_core::context_builder::WindowAggregator;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_spec::traits::PrivacySanitizer;
use aios_spec::{
    FsAccessEvent, FsAccessType, NotificationAction, NotificationInteractionRawEvent,
    NotificationRawEvent, RawEvent,
};

/// One leak-test row: a raw input and the substrings that must not appear in
/// any serialization downstream.
struct Case {
    name: &'static str,
    raw: RawEvent,
    forbidden: Vec<&'static str>,
}

fn notif(timestamp: i64, package: &str, title: &str, text: &str) -> RawEvent {
    RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: timestamp,
        package_name: package.to_string(),
        category: Some("msg".into()),
        channel_id: None,
        raw_title: title.into(),
        raw_text: text.into(),
        title_hint: None,
        text_hint: None,
        semantic_hints: vec![],
        is_ongoing: false,
        group_key: None,
        has_picture: false,
    })
}

fn fs(timestamp: i64, path: &str) -> RawEvent {
    RawEvent::FileSystemAccess(FsAccessEvent {
        timestamp_ms: timestamp,
        pid: 42,
        uid: 10123,
        file_path: path.into(),
        access_type: FsAccessType::OpenRead,
        bytes_transferred: Some(4096),
    })
}

fn leak_cases() -> Vec<Case> {
    vec![
        Case {
            name: "chinese-notification-title-and-body",
            raw: notif(
                1000,
                "com.tencent.mm",
                "王小明",
                "李华发来: 工资条 2024Q3.xlsx",
            ),
            forbidden: vec!["王小明", "李华发来", "工资条", "2024Q3.xlsx"],
        },
        Case {
            name: "verification-code-body",
            raw: notif(
                2000,
                "com.bank.app",
                "OTP",
                "Your one-time code is 987654, do not share",
            ),
            forbidden: vec!["987654", "do not share", "one-time code is"],
        },
        Case {
            name: "phone-number-body",
            raw: notif(
                3000,
                "com.android.dialer",
                "未接来电",
                "+86 138 1234 5678 of 18s",
            ),
            forbidden: vec!["138 1234 5678", "+86 138", "of 18s"],
        },
        Case {
            name: "credentials-style-body",
            raw: notif(
                4000,
                "com.example.email",
                "Login",
                "user@example.com pw=hunter2",
            ),
            forbidden: vec!["hunter2", "user@example.com", "pw=hunter2"],
        },
        Case {
            name: "fs-secret-document-path",
            raw: fs(5000, "/data/user/0/com.example/files/secret_invoice_q3.pdf"),
            forbidden: vec![
                "secret_invoice_q3",
                "/data/user/0/com.example",
                "secret_invoice_q3.pdf",
            ],
        },
        Case {
            name: "fs-photo-path",
            raw: fs(
                6000,
                "/storage/emulated/0/DCIM/Camera/IMG_20260607_123456.jpg",
            ),
            forbidden: vec![
                "IMG_20260607_123456",
                "/storage/emulated/0/DCIM",
                "Camera/IMG_",
            ],
        },
    ]
}

#[test]
fn sanitizer_output_never_contains_raw_pii() {
    let sanitizer = DefaultPrivacyAirGap;
    for case in leak_cases() {
        let sanitized = sanitizer.sanitize(case.raw);
        let json = serde_json::to_string(&sanitized).expect("SanitizedEvent serializes");
        for needle in &case.forbidden {
            assert!(
                !json.contains(needle),
                "[{}] forbidden substring `{}` leaked into SanitizedEvent JSON:\n{}",
                case.name,
                needle,
                json,
            );
        }
    }
}

#[test]
fn aggregated_context_never_contains_raw_pii() {
    let sanitizer = DefaultPrivacyAirGap;
    for case in leak_cases() {
        let sanitized = sanitizer.sanitize(case.raw);
        let timestamp = sanitized.timestamp_ms;
        let mut agg = WindowAggregator::new(10, timestamp);
        agg.push(sanitized);
        let ctx = agg
            .close(timestamp + 10_000)
            .expect("non-empty window builds a StructuredContext");
        let json = serde_json::to_string(&ctx).expect("StructuredContext serializes");
        for needle in &case.forbidden {
            assert!(
                !json.contains(needle),
                "[{}] forbidden substring `{}` leaked into StructuredContext JSON:\n{}",
                case.name,
                needle,
                json,
            );
        }
    }
}

// ===== Explicit-passthrough signatures =====
//
// Some raw fields are deliberately preserved by the current sanitizer. These
// tests pin that behavior so a future change either updates the test together
// with the code (intentional) or fails CI here (regression).

#[test]
fn notification_source_package_is_deliberately_preserved() {
    let sanitizer = DefaultPrivacyAirGap;
    let raw = notif(1000, "com.tencent.mm", "x", "y");
    let json = serde_json::to_string(&sanitizer.sanitize(raw)).unwrap();
    assert!(
        json.contains("com.tencent.mm"),
        "package_name is intentionally part of SanitizedEvent — if you strip \
         it, update this test and the WindowAggregator summary logic together"
    );
}

#[test]
fn notification_interaction_strips_raw_notification_key() {
    // The NotificationInteraction arm of `DefaultPrivacyAirGap` used to
    // forward `notification_key` verbatim into `group_key`. The Android key
    // format `<userId>|<package>|<id>|<tag>|<uid>` exposes a user-controlled
    // tag (chat thread, contact name). That arm now drops the key — this
    // test pins the new behavior.
    let sanitizer = DefaultPrivacyAirGap;
    let key = "0|com.example|42|user-tag-containing-PII|10042";
    let raw = RawEvent::NotificationInteraction(NotificationInteractionRawEvent {
        timestamp_ms: 1000,
        package_name: "com.example".into(),
        notification_key: key.into(),
        action: NotificationAction::Tapped,
    });

    let sanitized = sanitizer.sanitize(raw);
    let json = serde_json::to_string(&sanitized).unwrap();
    assert!(
        !json.contains("user-tag-containing-PII"),
        "tag portion of notification_key must not leak; got:\n{json}",
    );
    assert!(
        !json.contains(key),
        "full notification_key must not leak; got:\n{json}",
    );

    // The package portion is deliberately preserved via `source_package`
    // and `app_package` — sanity-check that the structural surface still
    // works (downstream summary aggregation depends on it).
    assert!(json.contains("com.example"));

    // And `group_key` itself is now explicitly None.
    match sanitized.event_type {
        aios_spec::SanitizedEventType::Notification { group_key, .. } => {
            assert!(
                group_key.is_none(),
                "group_key must be None for NotificationInteraction"
            );
        },
        _ => panic!("expected Notification event_type"),
    }
}

#[test]
fn notification_posted_strips_raw_group_key() {
    let sanitizer = DefaultPrivacyAirGap;
    let group_key = "0|com.example|42|private-thread-alice|10042";
    let raw = RawEvent::NotificationPosted(NotificationRawEvent {
        timestamp_ms: 1000,
        package_name: "com.example".into(),
        category: Some("msg".into()),
        channel_id: None,
        raw_title: "".into(),
        raw_text: "".into(),
        title_hint: None,
        text_hint: None,
        semantic_hints: vec![],
        is_ongoing: false,
        group_key: Some(group_key.into()),
        has_picture: false,
    });

    let sanitized = sanitizer.sanitize(raw);
    let json = serde_json::to_string(&sanitized).unwrap();
    assert!(
        !json.contains("private-thread-alice"),
        "group_key tag must not leak; got:\n{json}",
    );
    assert!(
        !json.contains(group_key),
        "full group_key must not leak; got:\n{json}",
    );

    match sanitized.event_type {
        aios_spec::SanitizedEventType::Notification { group_key, .. } => {
            assert!(
                group_key.is_none(),
                "group_key must be None for NotificationPosted"
            );
        },
        _ => panic!("expected Notification event_type"),
    }
}
