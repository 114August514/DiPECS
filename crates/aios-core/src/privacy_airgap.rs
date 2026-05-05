//! 隐私脱敏引擎 — DiPECS 的核心安全边界
//!
//! 所有 `RawEvent` 在此处被转化为 `SanitizedEvent`。
//! 在此边界之后, 原始敏感数据 (通知正文、文件名、Binder 参数) 不可访问。

use aios_spec::traits::PrivacySanitizer;
use aios_spec::{
    AppTransitionRawEvent, BinderTxEvent, ExtensionCategory, FsAccessEvent, FsActivityType,
    InteractionType, NotificationRawEvent, ProcStateEvent, RawEvent, SanitizedEvent,
    SanitizedEventType, ScriptHint, SemanticHint, SourceTier, TextHint,
};
use uuid::Uuid;

/// 默认脱敏引擎
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
                    group_key: Some(e.notification_key),
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

// ===== 各类型脱敏逻辑 =====

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
                // 非交互类 Binder 调用不产生事件 (如 getPackageInfo 等查询)
                event_type: SanitizedEventType::InterAppInteraction {
                    source_package: None,
                    target_service: e.target_service,
                    interaction_type: InteractionType::ServiceBind, // fallback for non-matching
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
            group_key: e.group_key,
        },
        source_tier: SourceTier::PublicApi,
        app_package: Some(e.package_name),
        uid: None,
    }
}

// ===== 文本分析 (不保留原文) =====

fn analyze_text(text: &str) -> TextHint {
    let length_chars = text.chars().count();
    let is_emoji_only = !text.is_empty() && text.chars().all(is_emoji);

    let script = if text.is_empty() {
        ScriptHint::Unknown
    } else {
        let mut has_latin = false;
        let mut has_hanzi = false;
        let mut has_cyrillic = false;
        let mut has_arabic = false;

        for ch in text.chars() {
            match ch {
                '\u{0041}'..='\u{007A}' | '\u{00C0}'..='\u{024F}' => has_latin = true,
                '\u{4E00}'..='\u{9FFF}'
                | '\u{3400}'..='\u{4DBF}'
                | '\u{3000}'..='\u{303F}'
                | '\u{FF00}'..='\u{FFEF}' => has_hanzi = true,
                '\u{0400}'..='\u{04FF}' | '\u{0500}'..='\u{052F}' => has_cyrillic = true,
                '\u{0600}'..='\u{06FF}'
                | '\u{0750}'..='\u{077F}'
                | '\u{FB50}'..='\u{FDFF}'
                | '\u{FE70}'..='\u{FEFF}' => has_arabic = true,
                _ => {},
            }
        }

        let count = [has_latin, has_hanzi, has_cyrillic, has_arabic]
            .iter()
            .filter(|&&x| x)
            .count();
        match count {
            0 => ScriptHint::Unknown,
            1 if has_latin => ScriptHint::Latin,
            1 if has_hanzi => ScriptHint::Hanzi,
            1 if has_cyrillic => ScriptHint::Cyrillic,
            1 if has_arabic => ScriptHint::Arabic,
            _ => ScriptHint::Mixed,
        }
    };

    TextHint {
        length_chars,
        script,
        is_emoji_only,
    }
}

/// 从通知标题和正文中提取语义标签
///
/// 关键词匹配在本地完成, 不上传原文。
fn extract_semantic_hints(title: &str, text: &str) -> Vec<SemanticHint> {
    let combined = format!("{} {}", title, text).to_lowercase();
    let mut hints = Vec::new();

    // 文件相关
    if contains_any(
        &combined,
        &[
            "文件",
            "file",
            "pdf",
            "doc",
            "docx",
            "xls",
            "xlsx",
            "ppt",
            "pptx",
            "zip",
            "rar",
            "attachment",
            "附件",
        ],
    ) {
        hints.push(SemanticHint::FileMention);
    }
    // 图片相关
    if contains_any(
        &combined,
        &[
            "图片",
            "照片",
            "截图",
            "image",
            "photo",
            "screenshot",
            "jpg",
            "jpeg",
            "png",
            "gif",
            "webp",
            "相册",
        ],
    ) {
        hints.push(SemanticHint::ImageMention);
    }
    // 语音相关
    if contains_any(
        &combined,
        &[
            "语音", "voice", "audio", "mp3", "wav", "aac", "录音", "通话",
        ],
    ) {
        hints.push(SemanticHint::AudioMessage);
    }
    // 链接相关
    if contains_any(&combined, &["http", "https", "www.", "链接", "link", "url"]) {
        hints.push(SemanticHint::LinkAttachment);
    }
    // 被提及 (@我)
    if contains_any(
        &combined,
        &["@你", "@所有人", "提到了你", "mentioned you", "@"],
    ) {
        hints.push(SemanticHint::UserMentioned);
    }
    // 日历/会议
    if contains_any(
        &combined,
        &[
            "会议",
            "meeting",
            "calendar",
            "日历",
            "invitation",
            "邀请",
            "schedule",
            "日程",
        ],
    ) {
        hints.push(SemanticHint::CalendarInvitation);
    }
    // 金融/交易
    if contains_any(
        &combined,
        &[
            "支付",
            "付款",
            "转账",
            "payment",
            "transaction",
            "红包",
            "balance",
            "余额",
        ],
    ) {
        hints.push(SemanticHint::FinancialContext);
    }
    // 验证码
    if contains_any(
        &combined,
        &["验证码", "code", "otp", "验证", "verification", "captcha"],
    ) {
        hints.push(SemanticHint::VerificationCode);
    }

    hints
}

/// 从文件路径中推断扩展名类别
fn classify_extension(path: &str) -> ExtensionCategory {
    let lower = path.to_lowercase();
    let ext = std::path::Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "csv" | "odt"
        | "ods" | "odp" => ExtensionCategory::Document,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "bmp" | "svg" | "tiff" => {
            ExtensionCategory::Image
        },
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" | "wmv" | "3gp" => ExtensionCategory::Video,
        "mp3" | "wav" | "aac" | "flac" | "ogg" | "wma" | "m4a" | "opus" => ExtensionCategory::Audio,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "apk" | "aab" => {
            ExtensionCategory::Archive
        },
        "py" | "js" | "ts" | "rs" | "cpp" | "c" | "h" | "java" | "kt" | "swift" | "go" | "so"
        | "dylib" | "dll" => ExtensionCategory::Code,
        "" => ExtensionCategory::Unknown,
        _ => ExtensionCategory::Other,
    }
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(kw))
}

fn is_emoji(ch: char) -> bool {
    matches!(ch,
        '\u{1F600}'..='\u{1F64F}'   // Emoticons
        | '\u{1F300}'..='\u{1F5FF}' // Misc Symbols and Pictographs
        | '\u{1F680}'..='\u{1F6FF}' // Transport and Map
        | '\u{1F900}'..='\u{1F9FF}' // Supplemental Symbols and Pictographs
        | '\u{2600}'..='\u{26FF}'   // Misc symbols
        | '\u{2700}'..='\u{27BF}'   // Dingbats
        | '\u{FE00}'..='\u{FE0F}'   // Variation Selectors
        | '\u{200D}'                 // ZWJ
        | '\u{1F1E0}'..='\u{1F1FF}' // Flags
    )
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}
