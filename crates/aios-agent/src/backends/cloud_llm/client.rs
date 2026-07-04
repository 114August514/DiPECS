use std::time::{Duration, Instant};

use aios_spec::{DecisionBackendResult, DecisionRoute, IntentBatch, ModelInput, StructuredContext};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use super::config::{CloudLlmConfig, CloudProvider};
use super::translate::{idle_batch, parse_model_output, translate_intents};
use crate::DecisionBackend;

#[derive(Debug, Clone)]
pub struct CloudLlmBackend {
    config: CloudLlmConfig,
    client: Client,
}

impl CloudLlmBackend {
    pub fn try_new(config: CloudLlmConfig) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|error| format!("building HTTP client failed: {error}"))?;
        Ok(Self { config, client })
    }

    fn request_intents(&self, input: &ModelInput) -> Result<IntentBatch, String> {
        let context = &input.current_context;
        let request = self.build_request_body(input)?;

        let mut http = self
            .client
            .post(&self.config.endpoint)
            .json(&request)
            .header("Accept", "application/json");
        if let Some(api_key) = &self.config.api_key {
            http = http.bearer_auth(api_key);
        }

        let response = http
            .send()
            .map_err(|error| format!("request failed: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("HTTP {} from cloud LLM endpoint", status.as_u16()));
        }

        let payload: ChatCompletionResponse = response
            .json()
            .map_err(|error| format!("invalid response JSON: {error}"))?;
        let content = payload
            .first_text()
            .ok_or_else(|| "no completion content returned".to_string())?;
        let model_output = parse_model_output(&content)?;

        Ok(IntentBatch {
            window_id: context.window_id.clone(),
            intents: translate_intents(model_output.intents)?,
            generated_at_ms: context.window_end_ms,
            model: payload.model.unwrap_or_else(|| self.config.model.clone()),
        })
    }

    fn render_user_prompt(&self, input: &ModelInput) -> Result<String, String> {
        render_model_input_prompt(input)
    }

    fn build_request_body(&self, input: &ModelInput) -> Result<ChatCompletionRequest, String> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: self.config.system_prompt.clone(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: self.render_user_prompt(input)?,
            },
        ];
        Ok(ChatCompletionRequest {
            model: self.config.model.clone(),
            temperature: self.config.temperature,
            response_format: ChatResponseFormat {
                kind: "json_object".to_string(),
            },
            messages,
            reasoning_effort: self.config.reasoning_effort.clone(),
            provider_options: ProviderRequestOptions::from_config(&self.config),
        })
    }
}

impl DecisionBackend for CloudLlmBackend {
    fn evaluate(&self, context: &StructuredContext) -> DecisionBackendResult {
        let input = ModelInput::current_only(context.clone());
        self.evaluate_model_input(&input)
    }

    fn evaluate_model_input(&self, input: &ModelInput) -> DecisionBackendResult {
        let context = &input.current_context;
        let start = Instant::now();
        match self.request_intents(input) {
            Ok(intent_batch) => {
                let rationale_tags = intent_batch
                    .intents
                    .iter()
                    .flat_map(|intent| intent.rationale_tags.iter().cloned())
                    .collect();
                DecisionBackendResult {
                    route: DecisionRoute::CloudLlm,
                    intent_batch,
                    rationale_tags,
                    latency_us: start.elapsed().as_micros() as u64,
                    error: None,
                }
            },
            Err(error) => DecisionBackendResult {
                route: DecisionRoute::CloudLlm,
                intent_batch: idle_batch(context, "cloud-llm-error".to_string()),
                rationale_tags: vec!["cloud_llm_error".into()],
                latency_us: start.elapsed().as_micros() as u64,
                error: Some(error),
            },
        }
    }
}

pub(super) fn render_model_input_prompt(input: &ModelInput) -> Result<String, String> {
    let json = serde_json::to_string(input)
        .map_err(|error| format!("serializing ModelInput failed: {error}"))?;
    Ok(format!(
        "Generate DiPECS intents for this sanitized context plus behavior memory.\nwindow_id={}\nmodel_input_json={json}",
        input.current_context.window_id
    ))
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    temperature: f32,
    response_format: ChatResponseFormat,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(flatten)]
    provider_options: ProviderRequestOptions,
}

#[derive(Debug, Default, Serialize)]
struct ProviderRequestOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_thinking: Option<bool>,
}

impl ProviderRequestOptions {
    fn from_config(config: &CloudLlmConfig) -> Self {
        match config.provider {
            CloudProvider::DeepSeek => Self {
                thinking: config.enable_thinking.map(|enabled| ThinkingConfig {
                    kind: if enabled { "enabled" } else { "disabled" }.to_string(),
                }),
                enable_thinking: None,
            },
            CloudProvider::Qwen => Self {
                thinking: None,
                enable_thinking: config.enable_thinking,
            },
            CloudProvider::GenericOpenAiCompatible => Self::default(),
        }
    }
}

#[derive(Debug, Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Serialize)]
struct ChatResponseFormat {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    model: Option<String>,
    choices: Vec<ChatChoice>,
}

impl ChatCompletionResponse {
    fn first_text(&self) -> Option<String> {
        self.choices.first().and_then(|choice| {
            choice
                .message
                .content
                .clone()
                .filter(|content| !content.trim().is_empty())
                .or_else(|| choice.message.reasoning_content.clone())
        })
    }
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
    reasoning_content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::render_model_input_prompt;
    use aios_spec::{
        ContextSummary, ModelInput, RecentDecisionRecord, SanitizedEvent, SanitizedEventType,
        ScriptHint, SemanticHint, SourceTier, StructuredContext, TextHint, UserBehaviorProfile,
    };

    #[test]
    fn prompt_contains_model_memory_sections_without_raw_text() {
        let input = ModelInput {
            current_context: StructuredContext {
                window_id: "w-memory".into(),
                window_start_ms: 0,
                window_end_ms: 1000,
                duration_secs: 1,
                events: vec![SanitizedEvent {
                    event_id: "n1".into(),
                    timestamp_ms: 1,
                    event_type: SanitizedEventType::Notification {
                        source_package: "com.chat".into(),
                        category: None,
                        channel_id: None,
                        title_hint: TextHint {
                            length_chars: 12,
                            script: ScriptHint::Latin,
                            is_emoji_only: false,
                        },
                        text_hint: TextHint {
                            length_chars: 20,
                            script: ScriptHint::Latin,
                            is_emoji_only: false,
                        },
                        semantic_hints: vec![SemanticHint::FileMention],
                        is_ongoing: false,
                        group_key: None,
                    },
                    source_tier: SourceTier::PublicApi,
                    app_package: Some("com.chat".into()),
                    uid: None,
                }],
                summary: ContextSummary {
                    foreground_apps: vec!["com.chat".into()],
                    notified_apps: vec!["com.chat".into()],
                    all_semantic_hints: vec![SemanticHint::FileMention],
                    file_activity: vec![],
                    latest_system_status: None,
                    source_tier: SourceTier::PublicApi,
                },
            },
            behavior_profile: UserBehaviorProfile {
                user_id: None,
                summary: "usually opens docs after chat notifications".into(),
                observation_windows: 3,
                frequent_foreground_apps: vec![],
                frequent_notifying_apps: vec![],
                frequent_semantic_hints: vec![],
                action_successes: vec![],
                action_denials: vec![],
                action_failures: vec![],
                last_updated_window_id: Some("w-prev".into()),
            },
            recent_feedback: vec![RecentDecisionRecord {
                window_id: "w-prev".into(),
                window_start_ms: 0,
                window_end_ms: 1,
                foreground_apps: vec!["com.chat".into()],
                notified_apps: vec!["com.chat".into()],
                semantic_hints: vec![SemanticHint::FileMention],
                route: "CloudLlm".into(),
                model: "test".into(),
                intent_count: 1,
                rationale_tags: vec!["attachment".into()],
                backend_error: None,
                action_outcomes: vec![],
            }],
        };

        let prompt = render_model_input_prompt(&input).unwrap();
        assert!(prompt.contains("model_input_json="));
        assert!(prompt.contains("current_context"));
        assert!(prompt.contains("behavior_profile"));
        assert!(prompt.contains("recent_feedback"));
        assert!(prompt.contains("usually opens docs"));
        assert!(!prompt.contains("private raw notification body"));
    }
}
