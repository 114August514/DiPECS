//! LocalEvaluatorBackend - deterministic local intent evaluator.
//!
//! This backend is the local-only middle tier between hard-coded rules and a
//! cloud LLM. It deliberately avoids network access and raw event data. The
//! current implementation is a lightweight scorer over `StructuredContext`
//! signals, producing only low-risk actions allowed by the LocalEvaluator
//! capability profile.

use std::collections::HashMap;
use std::time::Instant;

use aios_spec::{
    ActionType, ActionUrgency, AppTransition, DecisionBackendResult, DecisionRoute,
    ExtensionCategory, FeedbackCorrectness, Intent, IntentBatch, IntentType, ModelInput,
    NetworkType, RecentDecisionRecord, RiskLevel, SanitizedEventType, ScreenState, SemanticHint,
    StructuredContext, SuggestedAction, UserBehaviorProfile,
};

use super::prefetch_target::default_prefetch_target;
use crate::{new_id, DecisionBackend};

pub struct LocalEvaluatorBackend;

const MAX_INTENTS_PER_WINDOW: usize = 5;
const LOW_BATTERY_THRESHOLD: u8 = 20;
const STRONG_CORRELATION_WINDOW_MS: i64 = 10_000;

const FILE_BASE_CONFIDENCE: f32 = 0.72;
const HOT_FILE_BASE_CONFIDENCE: f32 = 0.80;
const ONGOING_NOTIFICATION_CONFIDENCE: f32 = 0.66;
const ATTACHMENT_NOTIFICATION_CONFIDENCE: f32 = 0.74;
const FOREGROUND_TRANSITION_CONFIDENCE: f32 = 0.78;
const INTER_APP_CONFIDENCE: f32 = 0.80;
const LOW_BATTERY_CONFIDENCE: f32 = 0.84;
const IDLE_CONFIDENCE: f32 = 0.60;

const BOOST_HOT_FILE: f32 = 0.06;
const BOOST_FOREGROUND_NOTIFICATION_APP: f32 = 0.08;
const BOOST_LINK_ATTACHMENT: f32 = 0.03;
const BOOST_FOREGROUND_TRANSITION: f32 = 0.04;
const BOOST_REPEATED_PACKAGE: f32 = 0.04;
const BOOST_FREQUENT_PACKAGE: f32 = 0.03;
const BOOST_FILE_NOTIFICATION_STRONG: f32 = 0.06;
const BOOST_FILE_NOTIFICATION_WEAK: f32 = 0.03;
const BOOST_RECENT_FOREGROUND: f32 = 0.03;

const PACKAGE_BOOST_CAP: f32 = 0.08;
const BEHAVIOR_BOOST_CAP: f32 = 0.05;
const CORRELATION_BOOST_CAP: f32 = 0.08;

const PENALTY_LOW_BATTERY: f32 = -0.18;
const PENALTY_CELLULAR: f32 = -0.06;
const PENALTY_OFFLINE: f32 = -0.20;
const PENALTY_SCREEN_NONINTERACTIVE: f32 = -0.12;
const PENALTY_RECENT_POLICY_REJECTED: f32 = -0.12;
const PENALTY_RECENT_EXECUTION_FAILED: f32 = -0.08;

impl LocalEvaluatorBackend {
    fn evaluate_intents(&self, input: EvaluationInput<'_>) -> Vec<Intent> {
        let context = input.context;
        let signals = WindowSignals::from_context(context);
        let aggregation =
            WindowAggregation::from_context(context, input.behavior_profile, input.recent_feedback);
        let mut candidates = Vec::new();

        for event in &context.events {
            match &event.event_type {
                SanitizedEventType::FileActivity {
                    package_name,
                    extension_category,
                    is_hot_file,
                    ..
                } => {
                    let mut score = Score::new(if *is_hot_file {
                        HOT_FILE_BASE_CONFIDENCE
                    } else {
                        FILE_BASE_CONFIDENCE
                    });
                    if *is_hot_file {
                        score.add(BOOST_HOT_FILE, "local:boost:hot_file");
                    }
                    aggregation.apply_package_boost(package_name.as_deref(), &mut score);
                    aggregation.apply_file_notification_boost(
                        package_name.as_deref(),
                        event.timestamp_ms,
                        &mut score,
                    );
                    score.apply_window(&signals, false);
                    let actions = vec![SuggestedAction {
                        action_type: ActionType::PrefetchFile,
                        target: Some(default_prefetch_target(
                            extension_category,
                            package_name.as_deref(),
                        )),
                        urgency: ActionUrgency::IdleTime,
                    }];
                    aggregation.apply_action_feedback(&actions, &mut score);

                    candidates.push(IntentCandidate::new(
                        IntentType::HandleFile(extension_category.clone()),
                        score,
                        actions,
                        vec![
                            "local:file_activity".into(),
                            format!("local:extension:{extension_category:?}"),
                        ],
                    ));
                },
                SanitizedEventType::Notification {
                    source_package,
                    semantic_hints,
                    is_ongoing,
                    ..
                } if semantic_hints.contains(&SemanticHint::FileMention)
                    || semantic_hints.contains(&SemanticHint::ImageMention)
                    || semantic_hints.contains(&SemanticHint::LinkAttachment) =>
                {
                    let mut score = Score::new(if *is_ongoing {
                        ONGOING_NOTIFICATION_CONFIDENCE
                    } else {
                        ATTACHMENT_NOTIFICATION_CONFIDENCE
                    });
                    if signals.foreground_apps.contains(source_package) {
                        score.add(
                            BOOST_FOREGROUND_NOTIFICATION_APP,
                            "local:boost:foreground_notification_app",
                        );
                    }
                    if semantic_hints.contains(&SemanticHint::LinkAttachment) {
                        score.add(BOOST_LINK_ATTACHMENT, "local:boost:link_attachment");
                    }
                    aggregation.apply_package_boost(Some(source_package), &mut score);
                    aggregation.apply_notification_file_boost(
                        source_package,
                        event.timestamp_ms,
                        &mut score,
                    );
                    score.apply_window(&signals, true);
                    let actions = vec![SuggestedAction {
                        action_type: ActionType::PreWarmProcess,
                        target: Some(format!("pkg:{source_package}")),
                        urgency: ActionUrgency::Immediate,
                    }];
                    aggregation.apply_action_feedback(&actions, &mut score);

                    candidates.push(IntentCandidate::new(
                        IntentType::OpenApp(source_package.clone()),
                        score,
                        actions,
                        vec!["local:attachment_notification".into()],
                    ));
                },
                SanitizedEventType::AppTransition {
                    package_name,
                    transition: AppTransition::Foreground,
                    ..
                } => {
                    let mut score = Score::new(FOREGROUND_TRANSITION_CONFIDENCE);
                    score.add(BOOST_FOREGROUND_TRANSITION, "local:boost:foreground");
                    aggregation.apply_package_boost(Some(package_name), &mut score);
                    if aggregation.is_recent_foreground(package_name) {
                        score.add_grouped(
                            BoostGroup::Package,
                            BOOST_RECENT_FOREGROUND,
                            "local:boost:recent_foreground",
                        );
                    }
                    score.apply_window(&signals, true);
                    let actions = vec![
                        SuggestedAction {
                            action_type: ActionType::PreWarmProcess,
                            target: Some(format!("pkg:{package_name}")),
                            urgency: ActionUrgency::Immediate,
                        },
                        SuggestedAction {
                            action_type: ActionType::KeepAlive,
                            target: Some("work:collector_heartbeat".into()),
                            urgency: ActionUrgency::Immediate,
                        },
                    ];
                    aggregation.apply_action_feedback(&actions, &mut score);

                    candidates.push(IntentCandidate::new(
                        IntentType::SwitchToApp(package_name.clone()),
                        score,
                        actions,
                        vec!["local:foreground_transition".into()],
                    ));
                },
                SanitizedEventType::InterAppInteraction {
                    interaction_type:
                        aios_spec::InteractionType::ActivityLaunch
                        | aios_spec::InteractionType::ShareIntent,
                    ..
                } => {
                    let mut score = Score::new(INTER_APP_CONFIDENCE);
                    score.apply_window(&signals, false);
                    let actions = vec![SuggestedAction {
                        action_type: ActionType::PreWarmProcess,
                        target: Some("own:resources".into()),
                        urgency: ActionUrgency::Immediate,
                    }];
                    aggregation.apply_action_feedback(&actions, &mut score);

                    candidates.push(IntentCandidate::new(
                        IntentType::EnterContext("inter_app_interaction".into()),
                        score,
                        actions,
                        vec!["local:inter_app_interaction".into()],
                    ));
                },
                _ => {},
            }
        }

        if signals.low_battery && !signals.charging {
            let actions = vec![SuggestedAction {
                action_type: ActionType::ReleaseMemory,
                target: Some("cache:prefetch".into()),
                urgency: ActionUrgency::Immediate,
            }];
            let mut score = Score::with_tags(
                LOW_BATTERY_CONFIDENCE,
                vec!["local:signal:low_battery".into()],
            );
            aggregation.apply_action_feedback(&actions, &mut score);
            candidates.push(IntentCandidate::new(
                IntentType::Idle,
                score,
                actions,
                vec!["local:low_battery".into()],
            ));
        }

        let mut intents = finalize_candidates(candidates, &signals, MAX_INTENTS_PER_WINDOW);

        if intents.is_empty() {
            intents.push(Intent {
                intent_id: new_id(),
                intent_type: IntentType::Idle,
                confidence: IDLE_CONFIDENCE,
                risk_level: RiskLevel::Low,
                suggested_actions: vec![SuggestedAction {
                    action_type: ActionType::NoOp,
                    target: None,
                    urgency: ActionUrgency::IdleTime,
                }],
                rationale_tags: vec!["local:idle_window".into()],
            });
        }

        intents
    }
}

struct EvaluationInput<'a> {
    context: &'a StructuredContext,
    behavior_profile: Option<&'a UserBehaviorProfile>,
    recent_feedback: &'a [RecentDecisionRecord],
}

#[derive(Debug, Default)]
struct WindowAggregation {
    package_counts: HashMap<String, u32>,
    notification_counts: HashMap<String, u32>,
    file_package_counts: HashMap<String, u32>,
    notification_timestamps: HashMap<String, Vec<i64>>,
    file_timestamps: HashMap<String, Vec<i64>>,
    recent_foreground_app: Option<String>,
    frequent_foreground_apps: HashMap<String, u32>,
    frequent_notifying_apps: HashMap<String, u32>,
    recent_policy_rejections: HashMap<String, u32>,
    recent_execution_failures: HashMap<String, u32>,
}

impl WindowAggregation {
    fn from_context(
        context: &StructuredContext,
        behavior_profile: Option<&UserBehaviorProfile>,
        recent_feedback: &[RecentDecisionRecord],
    ) -> Self {
        let mut aggregation = Self::default();
        let mut recent_foreground: Option<(i64, String)> = None;

        for event in &context.events {
            match &event.event_type {
                SanitizedEventType::AppTransition {
                    package_name,
                    transition: AppTransition::Foreground,
                    ..
                } => {
                    increment_count(&mut aggregation.package_counts, package_name);
                    if recent_foreground
                        .as_ref()
                        .is_none_or(|(timestamp_ms, _)| event.timestamp_ms >= *timestamp_ms)
                    {
                        recent_foreground = Some((event.timestamp_ms, package_name.clone()));
                    }
                },
                SanitizedEventType::ProcessResource { .. } => {},
                SanitizedEventType::Notification { source_package, .. } => {
                    increment_count(&mut aggregation.package_counts, source_package);
                    increment_count(&mut aggregation.notification_counts, source_package);
                    push_timestamp(
                        &mut aggregation.notification_timestamps,
                        source_package,
                        event.timestamp_ms,
                    );
                },
                SanitizedEventType::FileActivity {
                    package_name: Some(pkg),
                    ..
                } => {
                    increment_count(&mut aggregation.package_counts, pkg);
                    increment_count(&mut aggregation.file_package_counts, pkg);
                    push_timestamp(&mut aggregation.file_timestamps, pkg, event.timestamp_ms);
                },
                SanitizedEventType::InterAppInteraction {
                    source_package: Some(pkg),
                    ..
                } => {
                    increment_count(&mut aggregation.package_counts, pkg);
                },
                _ => {},
            }
        }

        aggregation.recent_foreground_app = recent_foreground.map(|(_, package)| package);

        if let Some(profile) = behavior_profile {
            for (package, count) in &profile.frequent_foreground_apps {
                aggregation
                    .frequent_foreground_apps
                    .insert(package.clone(), *count);
            }
            for (package, count) in &profile.frequent_notifying_apps {
                aggregation
                    .frequent_notifying_apps
                    .insert(package.clone(), *count);
            }
        }

        for record in recent_feedback {
            for action in &record.action_outcomes {
                let key = feedback_action_key(&action.action_type, action.target.as_deref());
                if matches!(action.correctness, FeedbackCorrectness::PolicyRejected)
                    || action.terminal.contains("Denied")
                    || action.denial_reason.is_some()
                {
                    increment_count(&mut aggregation.recent_policy_rejections, &key);
                }
                if matches!(action.correctness, FeedbackCorrectness::ExecutionFailed)
                    || action.terminal.contains("Failed")
                    || action.error.is_some()
                {
                    increment_count(&mut aggregation.recent_execution_failures, &key);
                }
            }
        }

        aggregation
    }

    fn apply_package_boost(&self, package: Option<&str>, score: &mut Score) {
        let Some(package) = package else {
            return;
        };

        if self.package_counts.get(package).copied().unwrap_or(0) >= 2 {
            score.add_grouped(
                BoostGroup::Package,
                BOOST_REPEATED_PACKAGE,
                "local:boost:repeated_package_in_window",
            );
        }
        if self.frequent_foreground_apps.contains_key(package) {
            score.add_grouped(
                BoostGroup::Behavior,
                BOOST_FREQUENT_PACKAGE,
                "local:boost:frequent_foreground_app",
            );
        }
        if self.frequent_notifying_apps.contains_key(package) {
            score.add_grouped(
                BoostGroup::Behavior,
                BOOST_FREQUENT_PACKAGE,
                "local:boost:frequent_notifying_app",
            );
        }
    }

    fn apply_file_notification_boost(
        &self,
        package: Option<&str>,
        file_timestamp_ms: i64,
        score: &mut Score,
    ) {
        let Some(package) = package else {
            return;
        };

        match nearest_delta_ms(
            self.notification_timestamps.get(package).map(Vec::as_slice),
            file_timestamp_ms,
        ) {
            Some(delta) if delta <= STRONG_CORRELATION_WINDOW_MS => score.add_grouped(
                BoostGroup::Correlation,
                BOOST_FILE_NOTIFICATION_STRONG,
                "local:boost:file_notification_same_package_strong",
            ),
            Some(_) => score.add_grouped(
                BoostGroup::Correlation,
                BOOST_FILE_NOTIFICATION_WEAK,
                "local:boost:file_notification_same_package_weak",
            ),
            None => {},
        }
    }

    fn apply_notification_file_boost(
        &self,
        package: &str,
        notification_timestamp_ms: i64,
        score: &mut Score,
    ) {
        match nearest_delta_ms(
            self.file_timestamps.get(package).map(Vec::as_slice),
            notification_timestamp_ms,
        ) {
            Some(delta) if delta <= STRONG_CORRELATION_WINDOW_MS => score.add_grouped(
                BoostGroup::Correlation,
                BOOST_FILE_NOTIFICATION_STRONG,
                "local:boost:notification_file_activity_link_strong",
            ),
            Some(_) => score.add_grouped(
                BoostGroup::Correlation,
                BOOST_FILE_NOTIFICATION_WEAK,
                "local:boost:notification_file_activity_link_weak",
            ),
            None => {},
        }
    }

    fn is_recent_foreground(&self, package: &str) -> bool {
        self.recent_foreground_app.as_deref() == Some(package)
    }

    fn apply_action_feedback(&self, actions: &[SuggestedAction], score: &mut Score) {
        for action in actions {
            let key = feedback_action_key(
                action_type_name(&action.action_type),
                action.target.as_deref(),
            );
            if self.recent_policy_rejections.contains_key(&key) {
                score.add(
                    PENALTY_RECENT_POLICY_REJECTED,
                    "local:penalty:recent_policy_rejected",
                );
            }
            if self.recent_execution_failures.contains_key(&key) {
                score.add(
                    PENALTY_RECENT_EXECUTION_FAILED,
                    "local:penalty:recent_execution_failed",
                );
            }
        }
    }
}

fn increment_count(counts: &mut HashMap<String, u32>, key: &str) {
    *counts.entry(key.to_string()).or_insert(0) += 1;
}

fn push_timestamp(timestamps: &mut HashMap<String, Vec<i64>>, key: &str, timestamp_ms: i64) {
    timestamps
        .entry(key.to_string())
        .or_default()
        .push(timestamp_ms);
}

fn nearest_delta_ms(timestamps: Option<&[i64]>, timestamp_ms: i64) -> Option<i64> {
    timestamps?
        .iter()
        .map(|other| timestamp_ms.saturating_sub(*other).abs())
        .min()
}

fn feedback_action_key(action_type: &str, target: Option<&str>) -> String {
    format!("{}:{}", action_type, target.unwrap_or(""))
}

fn action_type_name(action_type: &ActionType) -> &'static str {
    match action_type {
        ActionType::PrefetchFile => "PrefetchFile",
        ActionType::PreWarmProcess => "PreWarmProcess",
        ActionType::KeepAlive => "KeepAlive",
        ActionType::ReleaseMemory => "ReleaseMemory",
        ActionType::NoOp => "NoOp",
    }
}

#[derive(Debug, Default)]
struct WindowSignals {
    foreground_apps: Vec<String>,
    low_battery: bool,
    charging: bool,
    cellular: bool,
    offline: bool,
    screen_noninteractive: bool,
}

impl WindowSignals {
    fn from_context(context: &StructuredContext) -> Self {
        let mut signals = Self {
            foreground_apps: context.summary.foreground_apps.clone(),
            ..Self::default()
        };

        if let Some(status) = &context.summary.latest_system_status {
            signals.low_battery = status
                .battery_pct
                .is_some_and(|pct| pct < LOW_BATTERY_THRESHOLD);
            signals.charging = status.is_charging;
            signals.cellular = matches!(status.network, NetworkType::Cellular);
            signals.offline = matches!(status.network, NetworkType::Offline);
        }

        for event in &context.events {
            match &event.event_type {
                SanitizedEventType::SystemStatus {
                    battery_pct,
                    is_charging,
                    network,
                    ..
                } => {
                    signals.low_battery |=
                        battery_pct.is_some_and(|pct| pct < LOW_BATTERY_THRESHOLD);
                    signals.charging |= *is_charging;
                    signals.cellular |= matches!(network, NetworkType::Cellular);
                    signals.offline |= matches!(network, NetworkType::Offline);
                },
                SanitizedEventType::Screen {
                    state:
                        ScreenState::NonInteractive
                        | ScreenState::KeyguardShown
                        | ScreenState::KeyguardHidden,
                } => {
                    signals.screen_noninteractive = true;
                },
                _ => {},
            }
        }

        signals
    }
}

struct Score {
    value: f32,
    tags: Vec<String>,
    package_boost: f32,
    behavior_boost: f32,
    correlation_boost: f32,
}

impl Score {
    fn new(value: f32) -> Self {
        Self {
            value,
            tags: Vec::new(),
            package_boost: 0.0,
            behavior_boost: 0.0,
            correlation_boost: 0.0,
        }
    }

    fn with_tags(value: f32, tags: Vec<String>) -> Self {
        Self {
            value,
            tags,
            package_boost: 0.0,
            behavior_boost: 0.0,
            correlation_boost: 0.0,
        }
    }

    fn add(&mut self, delta: f32, tag: &str) {
        self.value += delta;
        self.tags.push(tag.into());
    }

    fn add_grouped(&mut self, group: BoostGroup, delta: f32, tag: &str) {
        let remaining = match group {
            BoostGroup::Package => PACKAGE_BOOST_CAP - self.package_boost,
            BoostGroup::Behavior => BEHAVIOR_BOOST_CAP - self.behavior_boost,
            BoostGroup::Correlation => CORRELATION_BOOST_CAP - self.correlation_boost,
        };
        let applied = delta.min(remaining.max(0.0));
        if applied <= 0.0 {
            self.tags.push(format!("{tag}:capped"));
            return;
        }

        self.value += applied;
        match group {
            BoostGroup::Package => self.package_boost += applied,
            BoostGroup::Behavior => self.behavior_boost += applied,
            BoostGroup::Correlation => self.correlation_boost += applied,
        }
        self.tags.push(tag.into());
    }

    fn apply_window(&mut self, signals: &WindowSignals, user_visible_hint: bool) {
        if signals.low_battery && !signals.charging {
            self.add(PENALTY_LOW_BATTERY, "local:penalty:low_battery");
        }
        if signals.cellular {
            self.add(PENALTY_CELLULAR, "local:penalty:cellular_network");
        }
        if signals.offline {
            self.add(PENALTY_OFFLINE, "local:penalty:offline");
        }
        if signals.screen_noninteractive && user_visible_hint {
            self.add(
                PENALTY_SCREEN_NONINTERACTIVE,
                "local:penalty:screen_noninteractive",
            );
        }
        self.value = self.value.clamp(0.0, 0.99);
    }
}

#[derive(Clone, Copy)]
enum BoostGroup {
    Package,
    Behavior,
    Correlation,
}

struct IntentCandidate {
    intent_type: IntentType,
    confidence: f32,
    actions: Vec<SuggestedAction>,
    tags: Vec<String>,
}

impl IntentCandidate {
    fn new(
        intent_type: IntentType,
        score: Score,
        actions: Vec<SuggestedAction>,
        mut tags: Vec<String>,
    ) -> Self {
        tags.extend(score.tags);
        Self {
            intent_type,
            confidence: score.value,
            actions,
            tags,
        }
    }
}

fn finalize_candidates(
    mut candidates: Vec<IntentCandidate>,
    signals: &WindowSignals,
    max_intents: usize,
) -> Vec<Intent> {
    for candidate in &mut candidates {
        let original_len = candidate.actions.len();
        candidate
            .actions
            .retain(|action| is_policy_aware_action(action, signals));
        if candidate.actions.len() < original_len {
            candidate
                .tags
                .push("local:suppress:policy_aware_filter".into());
        }
    }

    candidates.retain(|candidate| !candidate.actions.is_empty());

    let mut deduped: HashMap<String, IntentCandidate> = HashMap::new();
    for candidate in candidates {
        let key = candidate_key(&candidate);
        match deduped.get_mut(&key) {
            Some(existing) if existing.confidence >= candidate.confidence => {
                merge_tags(&mut existing.tags, candidate.tags);
            },
            Some(existing) => {
                let mut replacement = candidate;
                merge_tags(&mut replacement.tags, existing.tags.clone());
                *existing = replacement;
            },
            None => {
                deduped.insert(key, candidate);
            },
        }
    }

    let mut candidates: Vec<_> = deduped.into_values().collect();
    candidates.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| candidate_key(a).cmp(&candidate_key(b)))
    });

    candidates
        .into_iter()
        .take(max_intents)
        .map(|candidate| Intent {
            intent_id: new_id(),
            intent_type: candidate.intent_type,
            confidence: candidate.confidence,
            risk_level: RiskLevel::Low,
            suggested_actions: candidate.actions,
            rationale_tags: candidate.tags,
        })
        .collect()
}

fn is_policy_aware_action(action: &SuggestedAction, signals: &WindowSignals) -> bool {
    match action.action_type {
        ActionType::PrefetchFile => {
            (!signals.low_battery || signals.charging)
                && !signals.offline
                && action
                    .target
                    .as_deref()
                    .is_some_and(is_safe_prefetch_target)
        },
        ActionType::PreWarmProcess => action
            .target
            .as_deref()
            .is_some_and(|target| is_safe_prewarm_target(target, signals)),
        ActionType::KeepAlive => action
            .target
            .as_deref()
            .is_none_or(|target| target.starts_with("work:")),
        ActionType::ReleaseMemory => action
            .target
            .as_deref()
            .is_none_or(|target| matches!(target, "cache:prefetch" | "cache:all")),
        ActionType::NoOp => action.target.is_none(),
    }
}

fn is_safe_prefetch_target(target: &str) -> bool {
    target.starts_with("url:https://") || target.starts_with("uri:content://")
}

fn is_safe_prewarm_target(target: &str, signals: &WindowSignals) -> bool {
    if signals.screen_noninteractive && (target.starts_with("pkg:") || target.starts_with("notif:"))
    {
        return false;
    }

    target.starts_with("own:")
        || target.starts_with("notif:")
        || target
            .strip_prefix("pkg:")
            .is_some_and(super::prefetch_target::looks_like_package_name)
}

fn candidate_key(candidate: &IntentCandidate) -> String {
    let mut parts = vec![intent_key(&candidate.intent_type)];
    for action in &candidate.actions {
        let action_type = &action.action_type;
        let target = action.target.as_deref().unwrap_or("");
        parts.push(format!("{action_type:?}:{target}"));
    }
    parts.join("|")
}

fn intent_key(intent_type: &IntentType) -> String {
    match intent_type {
        IntentType::OpenApp(pkg) => format!("open:{pkg}"),
        IntentType::SwitchToApp(pkg) => format!("switch:{pkg}"),
        IntentType::CheckNotification(pkg) => format!("notify:{pkg}"),
        IntentType::HandleFile(ext) => format!("file:{}", extension_key(ext)),
        IntentType::EnterContext(ctx) => format!("context:{ctx}"),
        IntentType::Idle => "idle".into(),
    }
}

fn extension_key(ext: &ExtensionCategory) -> &'static str {
    match ext {
        ExtensionCategory::Document => "document",
        ExtensionCategory::Image => "image",
        ExtensionCategory::Video => "video",
        ExtensionCategory::Audio => "audio",
        ExtensionCategory::Archive => "archive",
        ExtensionCategory::Code => "code",
        ExtensionCategory::Other => "other",
        ExtensionCategory::Unknown => "unknown",
    }
}

fn merge_tags(target: &mut Vec<String>, source: Vec<String>) {
    for tag in source {
        if !target.contains(&tag) {
            target.push(tag);
        }
    }
}

impl DecisionBackend for LocalEvaluatorBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        self.evaluate_input(EvaluationInput {
            context,
            behavior_profile: None,
            recent_feedback: &[],
        })
    }

    fn evaluate_model_input(&self, input: &ModelInput) -> DecisionBackendResult {
        self.evaluate_input(EvaluationInput {
            context: &input.current_context,
            behavior_profile: Some(&input.behavior_profile),
            recent_feedback: &input.recent_feedback,
        })
    }
}

impl LocalEvaluatorBackend {
    fn evaluate_input(&self, input: EvaluationInput<'_>) -> DecisionBackendResult {
        let start = Instant::now();
        let context = input.context;
        let intents = self.evaluate_intents(input);
        let intent_batch = IntentBatch {
            window_id: context.window_id.clone(),
            intents,
            generated_at_ms: context.window_end_ms,
            model: "local-evaluator-v0.1".into(),
        };
        let rationale_tags = intent_batch
            .intents
            .iter()
            .flat_map(|intent| intent.rationale_tags.iter().cloned())
            .collect();

        DecisionBackendResult {
            route: DecisionRoute::LocalEvaluator,
            intent_batch,
            rationale_tags,
            latency_us: start.elapsed().as_micros() as u64,
            error: None,
        }
    }
}
