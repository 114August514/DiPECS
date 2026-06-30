//! Privacy-preserving model memory built from sanitized windows and audit records.

use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use aios_spec::governance::{ActionState, AuditRecord};
use aios_spec::{
    ActionFeedbackRecord, ActionType, DecisionBackendResult, FeedbackCorrectness, ModelInput,
    RecentDecisionRecord, SemanticHint, StructuredContext, UserBehaviorProfile,
};
use serde::{Deserialize, Serialize};

const DEFAULT_RECENT_LIMIT: usize = 5;
const DEFAULT_TOP_LIMIT: usize = 8;
const DEFAULT_MOMENTUM_DECAY_MILLI: u16 = 900;
const DEFAULT_PREWARM_EFFECT_WINDOWS: u8 = 3;
const MIN_RECENT_LIMIT: usize = 1;
const MAX_RECENT_LIMIT: usize = 50;
const MIN_TOP_LIMIT: usize = 1;
const MAX_TOP_LIMIT: usize = 32;
const MIN_MOMENTUM_DECAY_MILLI: u16 = 0;
const MAX_MOMENTUM_DECAY_MILLI: u16 = 999;
const MIN_PREWARM_EFFECT_WINDOWS: u8 = 1;
const MAX_PREWARM_EFFECT_WINDOWS: u8 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMemoryConfig {
    pub recent_limit: usize,
    pub top_limit: usize,
    pub momentum_decay_milli: u16,
    pub prewarm_effect_windows: u8,
    pub persist_path: Option<PathBuf>,
}

impl Default for ModelMemoryConfig {
    fn default() -> Self {
        Self {
            recent_limit: DEFAULT_RECENT_LIMIT,
            top_limit: DEFAULT_TOP_LIMIT,
            momentum_decay_milli: DEFAULT_MOMENTUM_DECAY_MILLI,
            prewarm_effect_windows: DEFAULT_PREWARM_EFFECT_WINDOWS,
            persist_path: None,
        }
    }
}

impl ModelMemoryConfig {
    pub fn from_env() -> Self {
        let recent_limit = read_usize_env("DIPECS_MODEL_MEMORY_RECENT_WINDOWS")
            .unwrap_or(DEFAULT_RECENT_LIMIT)
            .clamp(MIN_RECENT_LIMIT, MAX_RECENT_LIMIT);
        let top_limit = read_usize_env("DIPECS_MODEL_MEMORY_TOP_K")
            .unwrap_or(DEFAULT_TOP_LIMIT)
            .clamp(MIN_TOP_LIMIT, MAX_TOP_LIMIT);
        let momentum_decay_milli = read_u16_env("DIPECS_MODEL_MEMORY_MOMENTUM_DECAY_MILLI")
            .unwrap_or(DEFAULT_MOMENTUM_DECAY_MILLI)
            .clamp(MIN_MOMENTUM_DECAY_MILLI, MAX_MOMENTUM_DECAY_MILLI);
        let prewarm_effect_windows = read_u8_env("DIPECS_PREWARM_EFFECT_WINDOWS")
            .unwrap_or(DEFAULT_PREWARM_EFFECT_WINDOWS)
            .clamp(MIN_PREWARM_EFFECT_WINDOWS, MAX_PREWARM_EFFECT_WINDOWS);
        let persist_path = std::env::var("DIPECS_MODEL_MEMORY_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from);
        Self {
            recent_limit,
            top_limit,
            momentum_decay_milli,
            prewarm_effect_windows,
            persist_path,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMemoryStore {
    recent_limit: usize,
    top_limit: usize,
    #[serde(default = "default_momentum_decay_milli")]
    momentum_decay_milli: u16,
    #[serde(default = "default_prewarm_effect_windows")]
    prewarm_effect_windows: u8,
    llm_summary: Option<String>,
    observation_windows: u32,
    foreground_counts: BTreeMap<String, u32>,
    notifying_counts: BTreeMap<String, u32>,
    semantic_hint_counts: BTreeMap<String, u32>,
    action_success_counts: BTreeMap<String, u32>,
    action_denial_counts: BTreeMap<String, u32>,
    action_failure_counts: BTreeMap<String, u32>,
    #[serde(default)]
    foreground_momentum: BTreeMap<String, f64>,
    #[serde(default)]
    notifying_momentum: BTreeMap<String, f64>,
    #[serde(default)]
    semantic_hint_momentum: BTreeMap<String, f64>,
    #[serde(default)]
    action_success_momentum: BTreeMap<String, f64>,
    #[serde(default)]
    action_denial_momentum: BTreeMap<String, f64>,
    #[serde(default)]
    action_failure_momentum: BTreeMap<String, f64>,
    #[serde(default)]
    prewarm_hit_counts: BTreeMap<String, u32>,
    #[serde(default)]
    prewarm_miss_counts: BTreeMap<String, u32>,
    #[serde(default)]
    prewarm_hit_momentum: BTreeMap<String, f64>,
    #[serde(default)]
    prewarm_miss_momentum: BTreeMap<String, f64>,
    #[serde(default)]
    pending_prewarms: Vec<PendingPrewarm>,
    last_updated_window_id: Option<String>,
    recent: VecDeque<RecentDecisionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingPrewarm {
    window_id: String,
    target: String,
    package: String,
    windows_observed: u8,
}

impl Default for ModelMemoryStore {
    fn default() -> Self {
        Self::with_config(&ModelMemoryConfig::default())
    }
}

impl ModelMemoryStore {
    pub fn new(recent_limit: usize) -> Self {
        Self::with_config(&ModelMemoryConfig {
            recent_limit,
            ..ModelMemoryConfig::default()
        })
    }

    pub fn with_config(config: &ModelMemoryConfig) -> Self {
        Self {
            recent_limit: config
                .recent_limit
                .clamp(MIN_RECENT_LIMIT, MAX_RECENT_LIMIT),
            top_limit: config.top_limit.clamp(MIN_TOP_LIMIT, MAX_TOP_LIMIT),
            momentum_decay_milli: config
                .momentum_decay_milli
                .clamp(MIN_MOMENTUM_DECAY_MILLI, MAX_MOMENTUM_DECAY_MILLI),
            prewarm_effect_windows: config
                .prewarm_effect_windows
                .clamp(MIN_PREWARM_EFFECT_WINDOWS, MAX_PREWARM_EFFECT_WINDOWS),
            llm_summary: None,
            observation_windows: 0,
            foreground_counts: BTreeMap::new(),
            notifying_counts: BTreeMap::new(),
            semantic_hint_counts: BTreeMap::new(),
            action_success_counts: BTreeMap::new(),
            action_denial_counts: BTreeMap::new(),
            action_failure_counts: BTreeMap::new(),
            foreground_momentum: BTreeMap::new(),
            notifying_momentum: BTreeMap::new(),
            semantic_hint_momentum: BTreeMap::new(),
            action_success_momentum: BTreeMap::new(),
            action_denial_momentum: BTreeMap::new(),
            action_failure_momentum: BTreeMap::new(),
            prewarm_hit_counts: BTreeMap::new(),
            prewarm_miss_counts: BTreeMap::new(),
            prewarm_hit_momentum: BTreeMap::new(),
            prewarm_miss_momentum: BTreeMap::new(),
            pending_prewarms: Vec::new(),
            last_updated_window_id: None,
            recent: VecDeque::new(),
        }
    }

    pub fn load_or_default(config: &ModelMemoryConfig) -> Self {
        let Some(path) = &config.persist_path else {
            return Self::with_config(config);
        };
        match Self::load_from_path(path) {
            Ok(mut memory) => {
                memory.apply_config(config);
                memory
            },
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "model memory load failed; starting fresh");
                Self::with_config(config)
            },
        }
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, String> {
        let file = File::open(path.as_ref())
            .map_err(|error| format!("open model memory {}: {error}", path.as_ref().display()))?;
        let reader = BufReader::new(file);
        serde_json::from_reader(reader)
            .map_err(|error| format!("parse model memory {}: {error}", path.as_ref().display()))
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    format!("create model memory dir {}: {error}", parent.display())
                })?;
            }
        }
        let file = File::create(path)
            .map_err(|error| format!("create model memory {}: {error}", path.display()))?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)
            .map_err(|error| format!("write model memory {}: {error}", path.display()))
    }

    pub fn persist_if_configured(&self, config: &ModelMemoryConfig) {
        if let Some(path) = &config.persist_path {
            if let Err(error) = self.save_to_path(path) {
                tracing::warn!(path = %path.display(), error = %error, "model memory persist failed");
            }
        }
    }

    pub fn set_llm_summary(&mut self, summary: impl Into<String>) {
        let summary = summary.into();
        if !summary.trim().is_empty() {
            self.llm_summary = Some(summary);
        }
    }

    pub fn observation_windows(&self) -> u32 {
        self.observation_windows
    }

    pub fn recent_feedback(&self) -> Vec<RecentDecisionRecord> {
        self.recent.iter().cloned().collect()
    }

    pub fn model_input(&self, current_context: &StructuredContext) -> ModelInput {
        ModelInput {
            current_context: current_context.clone(),
            behavior_profile: self.behavior_profile(),
            recent_feedback: self.recent.iter().cloned().collect(),
        }
    }

    pub fn observe_window(
        &mut self,
        context: &StructuredContext,
        decision: &DecisionBackendResult,
        audit_records: &[AuditRecord],
    ) {
        self.resolve_pending_prewarms(&context.summary.foreground_apps);
        self.decay_momentum();

        self.observation_windows = self.observation_windows.saturating_add(1);
        self.last_updated_window_id = Some(context.window_id.clone());

        for package in &context.summary.foreground_apps {
            increment(&mut self.foreground_counts, package.clone());
            add_momentum(&mut self.foreground_momentum, package.clone());
        }
        for package in &context.summary.notified_apps {
            increment(&mut self.notifying_counts, package.clone());
            add_momentum(&mut self.notifying_momentum, package.clone());
        }
        for hint in &context.summary.all_semantic_hints {
            let hint = format!("{hint:?}");
            increment(&mut self.semantic_hint_counts, hint.clone());
            add_momentum(&mut self.semantic_hint_momentum, hint);
        }
        for record in audit_records {
            let action = format!("{:?}", record.action_type);
            match record.terminal {
                ActionState::Succeeded => {
                    increment(&mut self.action_success_counts, action.clone());
                    add_momentum(&mut self.action_success_momentum, action);
                },
                ActionState::DeniedByCapability
                | ActionState::DeniedByPolicy
                | ActionState::RejectedInvalidSchema => {
                    increment(&mut self.action_denial_counts, action.clone());
                    add_momentum(&mut self.action_denial_momentum, action);
                },
                ActionState::Failed => {
                    increment(&mut self.action_failure_counts, action.clone());
                    add_momentum(&mut self.action_failure_momentum, action);
                },
                _ => {},
            }
        }

        let feedback = RecentDecisionRecord {
            window_id: context.window_id.clone(),
            window_start_ms: context.window_start_ms,
            window_end_ms: context.window_end_ms,
            foreground_apps: context.summary.foreground_apps.clone(),
            notified_apps: context.summary.notified_apps.clone(),
            semantic_hints: context.summary.all_semantic_hints.clone(),
            route: format!("{:?}", decision.route),
            model: decision.intent_batch.model.clone(),
            intent_count: decision.intent_batch.intents.len() as u32,
            rationale_tags: decision.rationale_tags.clone(),
            backend_error: decision.error.clone(),
            action_outcomes: audit_records.iter().map(action_feedback).collect(),
        };
        self.recent.push_back(feedback);
        while self.recent.len() > self.recent_limit {
            self.recent.pop_front();
        }

        for record in audit_records {
            self.register_pending_prewarm(context, record);
        }
    }
    pub fn behavior_profile(&self) -> UserBehaviorProfile {
        let frequent_foreground_apps = top_scores(&self.foreground_momentum, self.top_limit)
            .unwrap_or_else(|| top_counts(&self.foreground_counts, self.top_limit));
        let frequent_notifying_apps = top_scores(&self.notifying_momentum, self.top_limit)
            .unwrap_or_else(|| top_counts(&self.notifying_counts, self.top_limit));
        let frequent_semantic_hints =
            top_semantic_hint_scores(&self.semantic_hint_momentum, self.top_limit)
                .unwrap_or_else(|| top_semantic_hints(&self.semantic_hint_counts, self.top_limit));
        let action_successes = top_scores(&self.action_success_momentum, self.top_limit)
            .unwrap_or_else(|| top_counts(&self.action_success_counts, self.top_limit));
        let action_denials = top_scores(&self.action_denial_momentum, self.top_limit)
            .unwrap_or_else(|| top_counts(&self.action_denial_counts, self.top_limit));
        let action_failures = top_scores(&self.action_failure_momentum, self.top_limit)
            .unwrap_or_else(|| top_counts(&self.action_failure_counts, self.top_limit));
        let prewarm_hits = top_scores(&self.prewarm_hit_momentum, self.top_limit)
            .unwrap_or_else(|| top_counts(&self.prewarm_hit_counts, self.top_limit));
        let prewarm_misses = top_scores(&self.prewarm_miss_momentum, self.top_limit)
            .unwrap_or_else(|| top_counts(&self.prewarm_miss_counts, self.top_limit));
        let local_summary = render_summary(SummaryRenderInput {
            windows: self.observation_windows,
            momentum_decay_milli: self.momentum_decay_milli,
            prewarm_effect_windows: self.prewarm_effect_windows,
            foreground: &frequent_foreground_apps,
            notifying: &frequent_notifying_apps,
            hints: &frequent_semantic_hints,
            successes: &action_successes,
            denials: &action_denials,
            failures: &action_failures,
            prewarm_hits: &prewarm_hits,
            prewarm_misses: &prewarm_misses,
        });
        let summary = match &self.llm_summary {
            Some(llm) => format!("llm_summary={llm}; local_counters={local_summary}"),
            None => local_summary,
        };

        UserBehaviorProfile {
            summary,
            observation_windows: self.observation_windows,
            frequent_foreground_apps,
            frequent_notifying_apps,
            frequent_semantic_hints,
            action_successes,
            action_denials,
            action_failures,
            last_updated_window_id: self.last_updated_window_id.clone(),
        }
    }
    fn apply_config(&mut self, config: &ModelMemoryConfig) {
        self.recent_limit = config
            .recent_limit
            .clamp(MIN_RECENT_LIMIT, MAX_RECENT_LIMIT);
        self.top_limit = config.top_limit.clamp(MIN_TOP_LIMIT, MAX_TOP_LIMIT);
        self.momentum_decay_milli = config
            .momentum_decay_milli
            .clamp(MIN_MOMENTUM_DECAY_MILLI, MAX_MOMENTUM_DECAY_MILLI);
        self.prewarm_effect_windows = config
            .prewarm_effect_windows
            .clamp(MIN_PREWARM_EFFECT_WINDOWS, MAX_PREWARM_EFFECT_WINDOWS);
        while self.recent.len() > self.recent_limit {
            self.recent.pop_front();
        }
    }

    fn decay_momentum(&mut self) {
        let decay = f64::from(self.momentum_decay_milli) / 1000.0;
        decay_map(&mut self.foreground_momentum, decay);
        decay_map(&mut self.notifying_momentum, decay);
        decay_map(&mut self.semantic_hint_momentum, decay);
        decay_map(&mut self.action_success_momentum, decay);
        decay_map(&mut self.action_denial_momentum, decay);
        decay_map(&mut self.action_failure_momentum, decay);
        decay_map(&mut self.prewarm_hit_momentum, decay);
        decay_map(&mut self.prewarm_miss_momentum, decay);
    }

    fn resolve_pending_prewarms(&mut self, foreground_apps: &[String]) {
        let mut still_pending = Vec::new();
        let pending = std::mem::take(&mut self.pending_prewarms);
        for mut prewarm in pending {
            prewarm.windows_observed = prewarm.windows_observed.saturating_add(1);
            if foreground_apps
                .iter()
                .any(|package| package == &prewarm.package)
            {
                self.record_prewarm_attribution(
                    &prewarm,
                    FeedbackCorrectness::PredictionHit,
                    format!(
                        "prewarm target {} opened within {} observed window(s)",
                        prewarm.package, prewarm.windows_observed
                    ),
                    true,
                );
            } else if prewarm.windows_observed >= self.prewarm_effect_windows {
                self.record_prewarm_attribution(
                    &prewarm,
                    FeedbackCorrectness::PredictionMiss,
                    format!(
                        "prewarm target {} was not opened within {} window(s)",
                        prewarm.package, self.prewarm_effect_windows
                    ),
                    false,
                );
            } else {
                still_pending.push(prewarm);
            }
        }
        self.pending_prewarms = still_pending;
    }

    fn record_prewarm_attribution(
        &mut self,
        prewarm: &PendingPrewarm,
        correctness: FeedbackCorrectness,
        evidence: String,
        hit: bool,
    ) {
        if hit {
            increment(&mut self.prewarm_hit_counts, prewarm.package.clone());
            add_momentum(&mut self.prewarm_hit_momentum, prewarm.package.clone());
        } else {
            increment(&mut self.prewarm_miss_counts, prewarm.package.clone());
            add_momentum(&mut self.prewarm_miss_momentum, prewarm.package.clone());
        }

        for recent in &mut self.recent {
            if recent.window_id != prewarm.window_id {
                continue;
            }
            for outcome in &mut recent.action_outcomes {
                if outcome.action_type == "PreWarmProcess"
                    && outcome.target.as_deref() == Some(prewarm.target.as_str())
                {
                    outcome.correctness = correctness;
                    outcome.correctness_evidence = evidence.clone();
                    outcome.outcome_summary = Some(if hit {
                        format!("prewarm_effect_hit:{}", prewarm.package)
                    } else {
                        format!("prewarm_effect_miss:{}", prewarm.package)
                    });
                }
            }
        }
    }

    fn register_pending_prewarm(&mut self, context: &StructuredContext, record: &AuditRecord) {
        if record.action_type != ActionType::PreWarmProcess
            || record.terminal != ActionState::Succeeded
        {
            return;
        }
        let Some(target) = record.target.as_deref() else {
            return;
        };
        let Some(package) = normalize_prewarm_package(target) else {
            return;
        };
        self.pending_prewarms.push(PendingPrewarm {
            window_id: context.window_id.clone(),
            target: target.to_string(),
            package,
            windows_observed: 0,
        });
    }
}

fn default_momentum_decay_milli() -> u16 {
    DEFAULT_MOMENTUM_DECAY_MILLI
}

fn default_prewarm_effect_windows() -> u8 {
    DEFAULT_PREWARM_EFFECT_WINDOWS
}

fn normalize_prewarm_package(target: &str) -> Option<String> {
    target
        .strip_prefix("pkg:")
        .filter(|package| !package.trim().is_empty())
        .map(|package| package.trim().to_string())
}

fn add_momentum(scores: &mut BTreeMap<String, f64>, key: String) {
    *scores.entry(key).or_insert(0.0) += 1.0;
}

fn decay_map(scores: &mut BTreeMap<String, f64>, decay: f64) {
    if decay <= 0.0 {
        scores.clear();
        return;
    }
    for value in scores.values_mut() {
        *value *= decay;
    }
    scores.retain(|_, value| *value >= 0.01);
}

fn top_scores(scores: &BTreeMap<String, f64>, limit: usize) -> Option<Vec<(String, u32)>> {
    if scores.is_empty() {
        return None;
    }
    let mut values: Vec<(String, f64)> = scores
        .iter()
        .map(|(key, score)| (key.clone(), *score))
        .collect();
    values.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    values.truncate(limit);
    Some(
        values
            .into_iter()
            .map(|(key, score)| (key, score.round().max(1.0) as u32))
            .collect(),
    )
}

fn top_semantic_hint_scores(
    scores: &BTreeMap<String, f64>,
    limit: usize,
) -> Option<Vec<(SemanticHint, u32)>> {
    top_scores(scores, limit).map(|values| {
        values
            .into_iter()
            .filter_map(|(name, count)| semantic_hint_from_debug(&name).map(|hint| (hint, count)))
            .collect()
    })
}
fn action_feedback(record: &AuditRecord) -> ActionFeedbackRecord {
    let (correctness, evidence) = infer_correctness(record);
    ActionFeedbackRecord {
        action_type: format!("{:?}", record.action_type),
        target: record.target.clone(),
        terminal: format!("{:?}", record.terminal),
        correctness,
        correctness_evidence: evidence,
        denial_reason: record.denial_reason.map(|reason| format!("{reason:?}")),
        error: record.error.clone(),
        outcome_summary: record
            .outcome
            .as_ref()
            .map(|outcome| outcome.summary.clone()),
    }
}

fn infer_correctness(record: &AuditRecord) -> (FeedbackCorrectness, String) {
    if record.action_type == ActionType::NoOp && record.terminal == ActionState::Succeeded {
        return (
            FeedbackCorrectness::NeutralNoOp,
            "NoOp succeeded; no behavioral preference inferred".into(),
        );
    }
    match record.terminal {
        ActionState::Succeeded => (
            FeedbackCorrectness::LikelyCorrect,
            "policy approved and adapter reported success".into(),
        ),
        ActionState::DeniedByCapability
        | ActionState::DeniedByPolicy
        | ActionState::RejectedInvalidSchema => (
            FeedbackCorrectness::PolicyRejected,
            "local policy or schema rejected the model suggestion".into(),
        ),
        ActionState::Failed => (
            FeedbackCorrectness::ExecutionFailed,
            "adapter attempted execution but returned failure".into(),
        ),
        _ => (
            FeedbackCorrectness::Unknown,
            "action did not reach a terminal feedback state".into(),
        ),
    }
}

fn increment(counts: &mut BTreeMap<String, u32>, key: String) {
    *counts.entry(key).or_insert(0) += 1;
}

fn top_counts(counts: &BTreeMap<String, u32>, limit: usize) -> Vec<(String, u32)> {
    let mut values: Vec<(String, u32)> = counts
        .iter()
        .map(|(key, count)| (key.clone(), *count))
        .collect();
    values.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    values.truncate(limit);
    values
}

fn top_semantic_hints(counts: &BTreeMap<String, u32>, limit: usize) -> Vec<(SemanticHint, u32)> {
    top_counts(counts, limit)
        .into_iter()
        .filter_map(|(name, count)| semantic_hint_from_debug(&name).map(|hint| (hint, count)))
        .collect()
}

fn semantic_hint_from_debug(name: &str) -> Option<SemanticHint> {
    match name {
        "FileMention" => Some(SemanticHint::FileMention),
        "ImageMention" => Some(SemanticHint::ImageMention),
        "AudioMessage" => Some(SemanticHint::AudioMessage),
        "LinkAttachment" => Some(SemanticHint::LinkAttachment),
        "UserMentioned" => Some(SemanticHint::UserMentioned),
        "CalendarInvitation" => Some(SemanticHint::CalendarInvitation),
        "FinancialContext" => Some(SemanticHint::FinancialContext),
        "VerificationCode" => Some(SemanticHint::VerificationCode),
        _ => None,
    }
}

struct SummaryRenderInput<'a> {
    windows: u32,
    momentum_decay_milli: u16,
    prewarm_effect_windows: u8,
    foreground: &'a [(String, u32)],
    notifying: &'a [(String, u32)],
    hints: &'a [(SemanticHint, u32)],
    successes: &'a [(String, u32)],
    denials: &'a [(String, u32)],
    failures: &'a [(String, u32)],
    prewarm_hits: &'a [(String, u32)],
    prewarm_misses: &'a [(String, u32)],
}

fn render_summary(input: SummaryRenderInput<'_>) -> String {
    format!(
        "observed_windows={}; momentum_decay_milli={}; prewarm_effect_window={}; foreground_apps={}; notifying_apps={}; semantic_hints={}; successful_actions={}; denied_actions={}; failed_actions={}; prewarm_effect_hits={}; prewarm_effect_misses={}",
        input.windows,
        input.momentum_decay_milli,
        input.prewarm_effect_windows,
        render_pairs(input.foreground),
        render_pairs(input.notifying),
        render_hint_pairs(input.hints),
        render_pairs(input.successes),
        render_pairs(input.denials),
        render_pairs(input.failures),
        render_pairs(input.prewarm_hits),
        render_pairs(input.prewarm_misses),
    )
}
fn render_pairs(values: &[(String, u32)]) -> String {
    if values.is_empty() {
        return "none".into();
    }
    values
        .iter()
        .map(|(key, count)| format!("{key}:{count}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn render_hint_pairs(values: &[(SemanticHint, u32)]) -> String {
    if values.is_empty() {
        return "none".into();
    }
    values
        .iter()
        .map(|(key, count)| format!("{key:?}:{count}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn read_u16_env(name: &str) -> Option<u16> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
}

fn read_u8_env(name: &str) -> Option<u8> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
}

fn read_usize_env(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy_airgap::DefaultPrivacyAirGap;
    use aios_spec::governance::{ActionCoord, ActionProposal, EffectClass};
    use aios_spec::intent::{ActionUrgency, SuggestedAction};
    use aios_spec::traits::PrivacySanitizer;
    use aios_spec::{
        AppTransition, AppTransitionRawEvent, ContextSummary, FsAccessEvent, FsAccessType,
        NotificationRawEvent, RawEvent, SourceTier,
    };

    #[test]
    fn memory_input_contains_recent_feedback_after_observation() {
        let ctx = test_context("w1");
        let decision = test_decision("w1");

        let mut memory = ModelMemoryStore::default();
        memory.observe_window(&ctx, &decision, &[]);
        let input = memory.model_input(&ctx);

        assert_eq!(input.behavior_profile.observation_windows, 1);
        assert_eq!(input.recent_feedback.len(), 1);
        assert!(input.behavior_profile.summary.contains("com.example:1"));
    }

    #[test]
    fn memory_round_trips_to_json_file() {
        let ctx = test_context("persist-window");
        let decision = test_decision("persist-window");
        let mut memory = ModelMemoryStore::new(3);
        memory.observe_window(&ctx, &decision, &[]);
        memory.set_llm_summary("opens docs after chat attachments");

        let path =
            std::env::temp_dir().join(format!("dipecs-model-memory-{}.json", uuid::Uuid::new_v4()));
        memory.save_to_path(&path).unwrap();
        let loaded = ModelMemoryStore::load_from_path(&path).unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(loaded.observation_windows(), 1);
        assert!(loaded
            .behavior_profile()
            .summary
            .contains("opens docs after chat attachments"));
    }

    #[test]
    fn feedback_correctness_marks_policy_denials() {
        let action = SuggestedAction {
            action_type: ActionType::PreWarmProcess,
            target: Some("pkg:com.example".into()),
            urgency: ActionUrgency::Immediate,
        };
        let proposal = ActionProposal {
            intent_id: "intent".into(),
            coord: ActionCoord {
                window_ordinal: 0,
                intent_ordinal: 0,
                action_ordinal: 0,
            },
            action,
            effect: EffectClass::LocalStateChange,
            proposed_at_ms: 1,
        };
        let mut record = AuditRecord::new(
            &proposal,
            aios_spec::DecisionRoute::CloudLlm,
            SourceTier::PublicApi,
        );
        record.transition(ActionState::DeniedByPolicy);

        let feedback = action_feedback(&record);
        assert_eq!(feedback.correctness, FeedbackCorrectness::PolicyRejected);
        assert!(feedback.correctness_evidence.contains("rejected"));
    }

    #[test]
    fn momentum_profile_prioritizes_recent_windows() {
        let config = ModelMemoryConfig {
            momentum_decay_milli: 200,
            ..ModelMemoryConfig::default()
        };
        let mut memory = ModelMemoryStore::with_config(&config);
        let decision = test_decision("w");

        for index in 0..3 {
            memory.observe_window(
                &test_context_with_foreground(&format!("old-{index}"), "com.old"),
                &decision,
                &[],
            );
        }
        memory.observe_window(
            &test_context_with_foreground("new", "com.new"),
            &decision,
            &[],
        );

        let profile = memory.behavior_profile();
        assert_eq!(profile.frequent_foreground_apps[0].0, "com.new");
        assert!(profile.summary.contains("momentum_decay_milli=200"));
    }

    #[test]
    fn prewarm_effect_hit_updates_recent_feedback() {
        let config = ModelMemoryConfig {
            prewarm_effect_windows: 2,
            ..ModelMemoryConfig::default()
        };
        let mut memory = ModelMemoryStore::with_config(&config);
        let decision = test_decision("prewarm");
        let record = succeeded_prewarm_record("pkg:com.target");

        memory.observe_window(
            &test_context_with_foreground("prewarm", "com.source"),
            &decision,
            &[record],
        );
        memory.observe_window(
            &test_context_with_foreground("hit", "com.target"),
            &decision,
            &[],
        );

        let recent = memory.recent_feedback();
        let prewarm_feedback = recent
            .iter()
            .find(|record| record.window_id == "prewarm")
            .and_then(|record| record.action_outcomes.first())
            .expect("prewarm feedback should remain in recent window");
        assert_eq!(
            prewarm_feedback.correctness,
            FeedbackCorrectness::PredictionHit
        );
        assert!(prewarm_feedback.correctness_evidence.contains("opened"));
        assert!(memory
            .behavior_profile()
            .summary
            .contains("prewarm_effect_hits=com.target:1"));
    }

    #[test]
    fn prewarm_effect_miss_updates_recent_feedback_after_horizon() {
        let config = ModelMemoryConfig {
            prewarm_effect_windows: 1,
            ..ModelMemoryConfig::default()
        };
        let mut memory = ModelMemoryStore::with_config(&config);
        let decision = test_decision("prewarm");
        let record = succeeded_prewarm_record("pkg:com.target");

        memory.observe_window(
            &test_context_with_foreground("prewarm", "com.source"),
            &decision,
            &[record],
        );
        memory.observe_window(
            &test_context_with_foreground("miss", "com.other"),
            &decision,
            &[],
        );

        let recent = memory.recent_feedback();
        let prewarm_feedback = recent
            .iter()
            .find(|record| record.window_id == "prewarm")
            .and_then(|record| record.action_outcomes.first())
            .expect("prewarm feedback should remain in recent window");
        assert_eq!(
            prewarm_feedback.correctness,
            FeedbackCorrectness::PredictionMiss
        );
        assert!(prewarm_feedback.correctness_evidence.contains("not opened"));
        assert!(memory
            .behavior_profile()
            .summary
            .contains("prewarm_effect_misses=com.target:1"));
    }
    #[test]
    fn model_input_does_not_contain_raw_notification_text_or_file_path() {
        let sanitizer = DefaultPrivacyAirGap;
        let notification = sanitizer.sanitize(RawEvent::NotificationPosted(NotificationRawEvent {
            timestamp_ms: 10,
            package_name: "com.chat".into(),
            category: None,
            channel_id: None,
            raw_title: "Alice private payroll".into(),
            raw_text: "secret verification 123456 at /sdcard/private/report.docx".into(),
            is_ongoing: false,
            group_key: Some("thread-private-alice".into()),
            has_picture: false,
        }));
        let file = sanitizer.sanitize(RawEvent::FileSystemAccess(FsAccessEvent {
            timestamp_ms: 11,
            pid: 1,
            uid: 2,
            file_path: "/sdcard/private/report.docx".into(),
            access_type: FsAccessType::OpenRead,
            bytes_transferred: Some(10),
        }));
        let ctx = StructuredContext {
            window_id: "privacy".into(),
            window_start_ms: 0,
            window_end_ms: 100,
            duration_secs: 1,
            events: vec![notification, file],
            summary: ContextSummary {
                foreground_apps: vec![],
                notified_apps: vec!["com.chat".into()],
                all_semantic_hints: vec![SemanticHint::VerificationCode],
                file_activity: vec![],
                latest_system_status: None,
                source_tier: SourceTier::PublicApi,
            },
        };

        let input = ModelMemoryStore::default().model_input(&ctx);
        let json = serde_json::to_string(&input).unwrap();

        assert!(!json.contains("Alice private payroll"));
        assert!(!json.contains("123456"));
        assert!(!json.contains("/sdcard/private/report.docx"));
        assert!(!json.contains("thread-private-alice"));
    }

    fn test_context(window_id: &str) -> StructuredContext {
        StructuredContext {
            window_id: window_id.into(),
            window_start_ms: 0,
            window_end_ms: 1000,
            duration_secs: 1,
            events: vec![DefaultPrivacyAirGap.sanitize(RawEvent::AppTransition(
                AppTransitionRawEvent {
                    timestamp_ms: 100,
                    package_name: "com.example".into(),
                    activity_class: None,
                    transition: AppTransition::Foreground,
                },
            ))],
            summary: ContextSummary {
                foreground_apps: vec!["com.example".into()],
                notified_apps: vec!["com.chat".into()],
                all_semantic_hints: vec![SemanticHint::FileMention],
                file_activity: vec![],
                latest_system_status: None,
                source_tier: SourceTier::PublicApi,
            },
        }
    }

    fn test_context_with_foreground(window_id: &str, package: &str) -> StructuredContext {
        let mut ctx = test_context(window_id);
        ctx.summary.foreground_apps = vec![package.into()];
        ctx.events =
            vec![
                DefaultPrivacyAirGap.sanitize(RawEvent::AppTransition(AppTransitionRawEvent {
                    timestamp_ms: 100,
                    package_name: package.into(),
                    activity_class: None,
                    transition: AppTransition::Foreground,
                })),
            ];
        ctx
    }

    fn succeeded_prewarm_record(target: &str) -> AuditRecord {
        let action = SuggestedAction {
            action_type: ActionType::PreWarmProcess,
            target: Some(target.into()),
            urgency: ActionUrgency::Immediate,
        };
        let proposal = ActionProposal {
            intent_id: "intent".into(),
            coord: ActionCoord {
                window_ordinal: 0,
                intent_ordinal: 0,
                action_ordinal: 0,
            },
            action,
            effect: EffectClass::LocalStateChange,
            proposed_at_ms: 1,
        };
        let mut record = AuditRecord::new(
            &proposal,
            aios_spec::DecisionRoute::CloudLlm,
            SourceTier::PublicApi,
        );
        record.transition(ActionState::Succeeded);
        record
    }
    fn test_decision(window_id: &str) -> DecisionBackendResult {
        DecisionBackendResult {
            route: aios_spec::DecisionRoute::RuleBased,
            intent_batch: aios_spec::IntentBatch {
                window_id: window_id.into(),
                intents: vec![],
                generated_at_ms: 1000,
                model: "test".into(),
            },
            rationale_tags: vec!["test".into()],
            latency_us: 0,
            error: None,
        }
    }
}
