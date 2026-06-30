//! Privacy sanitization boundary for DiPECS.
//!
//! Every `RawEvent` is converted into `SanitizedEvent` here. After this
//! boundary, raw sensitive data such as notification text, file paths, and
//! Binder parameters must not be accessible.
use aios_spec::traits::PrivacySanitizer;
use aios_spec::{
    AppTransitionRawEvent, BinderTxEvent, FsAccessEvent, FsActivityType, InteractionType,
    NotificationRawEvent, ProcStateEvent, RawEvent, SanitizedEvent, SanitizedEventType, ScriptHint,
    SourceTier, TextHint,
};
use uuid::Uuid;

use crate::text_analysis::{analyze_text, classify_extension, extract_semantic_hints};

/// Default privacy sanitizer.
pub struct DefaultPrivacyAirGap;

impl PrivacySanitizer for DefaultPrivacyAirGap {
    fn sanitize(&self, raw: RawEvent) -> SanitizedEvent {
        match raw {
            RawEvent::AppTransition(e) => sanitize_app_transition(e),
            RawEvent::BinderTransaction(e) => sanitize_binder(e),
            RawEvent::ProcStateChange(e) => sanitize_proc(e),
            RawEvent::FileSystemAccess(e) => sanitize_fs(e),
            RawEvent::NotificationPosted(e) => sanitize_notification(e),
            RawEvent::NotificationInteraction(e) => SanitizedEvent {
                event_id: new_id(),
                timestamp_ms: e.timestamp_ms,
                event_type: SanitizedEventType::Notification {
                    source_package: e.package_name.clone(),
                    category: None,
                    channel_id: None,
                    title_hint: TextHint {
                        length_chars: 0,
                        script: ScriptHint::Unknown,
                        is_emoji_only: false,
                    },
                    text_hint: TextHint {
                        length_chars: 0,
                        script: ScriptHint::Unknown,
                        is_emoji_only: false,
                    },
                    semantic_hints: vec![],
                    is_ongoing: false,
                    // The Android NotificationListenerService key has shape
                    // "<userId>|<package>|<id>|<tag>|<uid>"; the *tag*
                    // portion is user-controlled and can carry PII (contact
                    // names, chat thread IDs). No downstream consumer uses
                    // this key for dedup today, so the safe choice is to
                    // drop it at the air-gap. If a future feature genuinely
                    // needs cross-interaction correlation, replace this
                    // with a one-way hash of the key.
                    group_key: None,
                },
                source_tier: SourceTier::PublicApi,
                app_package: Some(e.package_name),
                uid: None,
            },
            RawEvent::ScreenState(e) => SanitizedEvent {
                event_id: new_id(),
                timestamp_ms: e.timestamp_ms,
                event_type: SanitizedEventType::Screen { state: e.state },
                source_tier: SourceTier::PublicApi,
                app_package: None,
                uid: None,
            },
            RawEvent::SystemState(e) => SanitizedEvent {
                event_id: new_id(),
                timestamp_ms: e.timestamp_ms,
                event_type: SanitizedEventType::SystemStatus {
                    battery_pct: e.battery_pct,
                    is_charging: e.is_charging,
                    network: e.network,
                    ringer_mode: e.ringer_mode,
                    location_type: e.location_type,
                    headphone_connected: e.headphone_connected,
                },
                source_tier: SourceTier::PublicApi,
                app_package: None,
                uid: None,
            },
        }
    }
}

// ===== Per-event sanitization logic =====

fn sanitize_app_transition(e: AppTransitionRawEvent) -> SanitizedEvent {
    SanitizedEvent {
        event_id: new_id(),
        timestamp_ms: e.timestamp_ms,
        event_type: SanitizedEventType::AppTransition {
            package_name: e.package_name.clone(),
            activity_class: e.activity_class,
            transition: e.transition,
        },
        source_tier: SourceTier::PublicApi,
        app_package: Some(e.package_name),
        uid: None,
    }
}

fn sanitize_binder(e: BinderTxEvent) -> SanitizedEvent {
    let interaction_type = match e.target_method.as_str() {
        m if m.contains("enqueueNotification") => InteractionType::NotifyPost,
        m if m.contains("startActivity") || m.contains("startActivityAsUser") => {
            InteractionType::ActivityLaunch
        },
        m if m.contains("share") || m.contains("sendIntent") => InteractionType::ShareIntent,
        m if m.contains("bindService") || m.contains("bindIsolatedService") => {
            InteractionType::ServiceBind
        },
        _ => {
            return SanitizedEvent {
                event_id: new_id(),
                timestamp_ms: e.timestamp_ms,
                event_type: SanitizedEventType::InterAppInteraction {
                    source_package: None,
                    target_service: e.target_service,
                    interaction_type: InteractionType::ServiceBind,
                },
                source_tier: SourceTier::Daemon,
                app_package: None,
                uid: Some(e.source_uid),
            };
        },
    };

    SanitizedEvent {
        event_id: new_id(),
        timestamp_ms: e.timestamp_ms,
        event_type: SanitizedEventType::InterAppInteraction {
            source_package: None,
            target_service: e.target_service,
            interaction_type,
        },
        source_tier: SourceTier::Daemon,
        app_package: None,
        uid: Some(e.source_uid),
    }
}

fn sanitize_proc(e: ProcStateEvent) -> SanitizedEvent {
    SanitizedEvent {
        event_id: new_id(),
        timestamp_ms: e.timestamp_ms,
        event_type: SanitizedEventType::ProcessResource {
            pid: e.pid,
            package_name: e.package_name.clone(),
            vm_rss_mb: (e.vm_rss_kb / 1024) as u32,
            vm_swap_mb: (e.vm_swap_kb / 1024) as u32,
            thread_count: e.threads,
            oom_score: e.oom_score,
        },
        source_tier: SourceTier::Daemon,
        app_package: e.package_name,
        uid: Some(e.uid),
    }
}

fn sanitize_fs(e: FsAccessEvent) -> SanitizedEvent {
    let ext_cat = classify_extension(&e.file_path);
    SanitizedEvent {
        event_id: new_id(),
        timestamp_ms: e.timestamp_ms,
        event_type: SanitizedEventType::FileActivity {
            package_name: None,
            extension_category: ext_cat,
            activity_type: match e.access_type {
                aios_spec::FsAccessType::OpenRead => FsActivityType::Read,
                aios_spec::FsAccessType::OpenWrite => FsActivityType::Write,
                aios_spec::FsAccessType::Create => FsActivityType::Create,
                aios_spec::FsAccessType::Delete => FsActivityType::Delete,
            },
            is_hot_file: false,
        },
        source_tier: SourceTier::Daemon,
        app_package: None,
        uid: Some(e.uid),
    }
}

fn sanitize_notification(e: NotificationRawEvent) -> SanitizedEvent {
    let title_hint = analyze_text(&e.raw_title);
    let text_hint = analyze_text(&e.raw_text);
    let semantic_hints = extract_semantic_hints(&e.raw_title, &e.raw_text);

    SanitizedEvent {
        event_id: new_id(),
        timestamp_ms: e.timestamp_ms,
        event_type: SanitizedEventType::Notification {
            source_package: e.package_name.clone(),
            category: e.category,
            channel_id: e.channel_id,
            title_hint,
            text_hint,
            semantic_hints,
            is_ongoing: e.is_ongoing,
            group_key: None,
        },
        source_tier: SourceTier::PublicApi,
        app_package: Some(e.package_name),
        uid: None,
    }
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}
