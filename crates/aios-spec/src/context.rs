//! Context window and model input types.
//!
//! Sanitized events are aggregated into `StructuredContext`. Cloud and local
//! model backends may also receive privacy-preserving behavior memory through
//! `ModelInput`.

use serde::{Deserialize, Serialize};

use crate::event::{
    ExtensionCategory, LocationType, NetworkType, RingerMode, SemanticHint, SourceTier,
};
use crate::sanitized::SanitizedEvent;

/// Sanitized context within a time window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredContext {
    pub window_id: String,
    pub window_start_ms: i64,
    pub window_end_ms: i64,
    pub duration_secs: u32,
    pub events: Vec<SanitizedEvent>,
    pub summary: ContextSummary,
}

/// Complete input made available to model backends.
///
/// `current_context` remains the only required field. The other fields are
/// derived after the privacy air-gap from prior sanitized windows and audit
/// records, so raw notification text, file paths, and other local PII still do
/// not cross the model boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInput {
    pub current_context: StructuredContext,
    pub behavior_profile: UserBehaviorProfile,
    pub recent_feedback: Vec<RecentDecisionRecord>,
}

impl ModelInput {
    pub fn current_only(current_context: StructuredContext) -> Self {
        Self {
            current_context,
            behavior_profile: UserBehaviorProfile::default(),
            recent_feedback: Vec::new(),
        }
    }
}

/// Rolling, privacy-preserving summary of observed user behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserBehaviorProfile {
    pub summary: String,
    pub observation_windows: u32,
    pub frequent_foreground_apps: Vec<(String, u32)>,
    pub frequent_notifying_apps: Vec<(String, u32)>,
    pub frequent_semantic_hints: Vec<(SemanticHint, u32)>,
    pub action_successes: Vec<(String, u32)>,
    pub action_denials: Vec<(String, u32)>,
    pub action_failures: Vec<(String, u32)>,
    pub last_updated_window_id: Option<String>,
}

/// Detailed but bounded record of recent model decisions and local outcomes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentDecisionRecord {
    pub window_id: String,
    pub window_start_ms: i64,
    pub window_end_ms: i64,
    pub foreground_apps: Vec<String>,
    pub notified_apps: Vec<String>,
    pub semantic_hints: Vec<SemanticHint>,
    pub route: String,
    pub model: String,
    pub intent_count: u32,
    pub rationale_tags: Vec<String>,
    pub backend_error: Option<String>,
    pub action_outcomes: Vec<ActionFeedbackRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionFeedbackRecord {
    pub action_type: String,
    pub target: Option<String>,
    pub terminal: String,
    pub correctness: FeedbackCorrectness,
    pub correctness_evidence: String,
    pub denial_reason: Option<String>,
    pub error: Option<String>,
    pub outcome_summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeedbackCorrectness {
    LikelyCorrect,
    PredictionHit,
    PredictionMiss,
    PolicyRejected,
    ExecutionFailed,
    NeutralNoOp,
    Unknown,
}

/// Aggregated summary of a context window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSummary {
    pub foreground_apps: Vec<String>,
    pub notified_apps: Vec<String>,
    pub all_semantic_hints: Vec<SemanticHint>,
    pub file_activity: Vec<(ExtensionCategory, u32)>,
    pub latest_system_status: Option<SystemStatusSnapshot>,
    pub source_tier: SourceTier,
}

/// Latest system status snapshot in a window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatusSnapshot {
    pub battery_pct: Option<u8>,
    pub is_charging: bool,
    pub network: NetworkType,
    pub ringer_mode: RingerMode,
    pub location_type: LocationType,
    pub headphone_connected: bool,
}
