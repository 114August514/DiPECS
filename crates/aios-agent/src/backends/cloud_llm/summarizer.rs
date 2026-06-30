use std::time::Duration;

use aios_spec::{RecentDecisionRecord, UserBehaviorProfile};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use super::config::{cloud_llm_enabled, CloudLlmConfig, CloudProvider};

const ENV_PROFILE_SUMMARY_ENABLED: &str = "DIPECS_PROFILE_SUMMARY_ENABLED";
const PROFILE_SUMMARY_SYSTEM_PROMPT: &str = r#"You summarize DiPECS user behavior memory.
Return only compact plain text, no markdown, no JSON.
Rules:
- Summarize stable habits from sanitized counters and recent feedback only.
- Mention action patterns that were likely correct, policy rejected, or failed.
- Do not invent app names or private content.
- Keep it under 80 words.
"#;

#[derive(Debug, Clone)]
pub struct ProfileSummarizer {
    config: CloudLlmConfig,
    client: Client,
}

impl ProfileSummarizer {
    pub fn from_env() -> Result<Option<Self>, String> {
        if !cloud_llm_enabled() || !profile_summary_enabled() {
            return Ok(None);
        }
        let config = CloudLlmConfig::from_env()?;
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|error| format!("building profile summarizer HTTP client failed: {error}"))?;
        Ok(Some(Self { config, client }))
    }

    pub fn summarize(
        &self,
        profile: &UserBehaviorProfile,
        recent_feedback: &[RecentDecisionRecord],
    ) -> Result<String, String> {
        let payload = ProfileSummaryInput {
            behavior_profile: profile,
            recent_feedback,
        };
        let user_json = serde_json::to_string(&payload)
            .map_err(|error| format!("serializing profile summary input failed: {error}"))?;
        let request = SummaryRequest {
            model: self.config.model.clone(),
            temperature: self.config.temperature,
            messages: vec![
                SummaryMessage {
                    role: "system".into(),
                    content: PROFILE_SUMMARY_SYSTEM_PROMPT.into(),
                },
                SummaryMessage {
                    role: "user".into(),
                    content: format!(
                        "Summarize this DiPECS behavior memory.\nprofile_summary_input_json={user_json}"
                    ),
                },
            ],
            provider_options: SummaryProviderOptions::from_config(&self.config),
        };

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
            .map_err(|error| format!("profile summary request failed: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "HTTP {} from profile summary endpoint",
                status.as_u16()
            ));
        }
        let payload: SummaryResponse = response
            .json()
            .map_err(|error| format!("invalid profile summary response JSON: {error}"))?;
        payload
            .first_text()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "no profile summary content returned".to_string())
    }
}

fn profile_summary_enabled() -> bool {
    matches!(
        std::env::var(ENV_PROFILE_SUMMARY_ENABLED).ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

#[derive(Debug, Serialize)]
struct ProfileSummaryInput<'a> {
    behavior_profile: &'a UserBehaviorProfile,
    recent_feedback: &'a [RecentDecisionRecord],
}

#[derive(Debug, Serialize)]
struct SummaryRequest {
    model: String,
    temperature: f32,
    messages: Vec<SummaryMessage>,
    #[serde(flatten)]
    provider_options: SummaryProviderOptions,
}

#[derive(Debug, Serialize)]
struct SummaryMessage {
    role: String,
    content: String,
}

#[derive(Debug, Default, Serialize)]
struct SummaryProviderOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<SummaryThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_thinking: Option<bool>,
}

impl SummaryProviderOptions {
    fn from_config(config: &CloudLlmConfig) -> Self {
        match config.provider {
            CloudProvider::DeepSeek => Self {
                thinking: config.enable_thinking.map(|enabled| SummaryThinkingConfig {
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
struct SummaryThinkingConfig {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct SummaryResponse {
    choices: Vec<SummaryChoice>,
}

impl SummaryResponse {
    fn first_text(&self) -> Option<String> {
        self.choices
            .first()
            .and_then(|choice| choice.message.content.clone())
    }
}

#[derive(Debug, Deserialize)]
struct SummaryChoice {
    message: SummaryMessageResponse,
}

#[derive(Debug, Deserialize)]
struct SummaryMessageResponse {
    content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_without_explicit_env() {
        std::env::remove_var(ENV_PROFILE_SUMMARY_ENABLED);
        assert!(!profile_summary_enabled());
    }
}
