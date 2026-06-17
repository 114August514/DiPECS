use std::time::{Duration, Instant};

use aios_spec::{DecisionBackendResult, DecisionRoute, IntentBatch, StructuredContext};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use super::config::{CloudLlmConfig, CloudProvider};
use super::translate::{idle_batch, parse_model_output, translate_intents};
use crate::DecisionBackend;

#[derive(Debug, Clone)]
pub(crate) struct CloudLlmBackend {
    config: CloudLlmConfig,
    client: Client,
}

impl CloudLlmBackend {
    pub(super) fn try_new(config: CloudLlmConfig) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|error| format!("building HTTP client failed: {error}"))?;
        Ok(Self { config, client })
    }

    fn request_intents(&self, context: &StructuredContext) -> Result<IntentBatch, String> {
        let request = self.build_request_body(context)?;

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

    fn render_user_prompt(&self, context: &StructuredContext) -> Result<String, String> {
        let json = serde_json::to_string(context)
            .map_err(|error| format!("serializing StructuredContext failed: {error}"))?;
        Ok(format!(
            "Generate DiPECS intents for this sanitized context.\nwindow_id={}\ncontext_json={json}",
            context.window_id
        ))
    }

    fn build_request_body(
        &self,
        context: &StructuredContext,
    ) -> Result<ChatCompletionRequest, String> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: self.config.system_prompt.clone(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: self.render_user_prompt(context)?,
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
        let start = Instant::now();
        match self.request_intents(context) {
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
        self.choices
            .first()
            .and_then(|choice| choice.message.content.clone())
    }
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
}
