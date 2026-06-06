//! CloudLlmBackend — OpenAI-compatible HTTP backend for optional cloud routing.

use std::env;
use std::time::Instant;

use aios_spec::{
    ActionType, ActionUrgency, DecisionBackendResult, DecisionRoute, ExtensionCategory, Intent,
    IntentBatch, IntentType, RiskLevel, StructuredContext, SuggestedAction,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::prefetch_target::{default_prefetch_target, looks_like_package_name};
use crate::{new_id, DecisionBackend};

const DEFAULT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_TEMPERATURE: f32 = 0.1;
const DEFAULT_DEEPSEEK_ENDPOINT: &str = "https://api.deepseek.com/chat/completions";
const DEFAULT_QWEN_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions";
const DEFAULT_SYSTEM_PROMPT: &str = r#"You are the decision backend for DiPECS.
Return only valid JSON with this shape:
{
  "intents": [
    {
      "intent_type": "OpenApp|SwitchToApp|CheckNotification|HandleFile|EnterContext|Idle",
      "target": "optional string",
      "extension_category": "Document|Image|Video|Audio|Archive|Code|Other|Unknown",
      "confidence": 0.0,
      "risk_level": "Low|Medium|High",
      "actions": [
        {
          "action_type": "PreWarmProcess|PrefetchFile|KeepAlive|ReleaseMemory|NoOp",
          "target": "optional string",
          "urgency": "Immediate|IdleTime|Deferred"
        }
      ],
      "rationale_tags": ["short_tag"]
    }
  ]
}

Rules:
- Return JSON only, no markdown fences.
- Use at most 3 intents.
- If uncertain, return one Idle intent with one NoOp action.
- For PrefetchFile, use a concrete Android bridge target when possible:
  `url:https://...` for network-accessible content or `uri:content://...` for
  persisted document/content-provider access.
- Use short snake_case rationale tags.
"#;

#[derive(Debug, Clone)]
pub enum CloudBackendState {
    Disabled,
    Misconfigured(String),
    Ready(CloudLlmBackend),
}

impl CloudBackendState {
    pub fn from_env() -> Self {
        if !read_bool_var("DIPECS_CLOUD_LLM_ENABLED").unwrap_or(false) {
            return Self::Disabled;
        }

        match CloudLlmConfig::from_env() {
            Ok(config) => match CloudLlmBackend::try_new(config) {
                Ok(backend) => Self::Ready(backend),
                Err(error) => Self::Misconfigured(error),
            },
            Err(error) => Self::Misconfigured(error),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CloudLlmBackend {
    config: CloudLlmConfig,
    client: Client,
}

impl CloudLlmBackend {
    fn try_new(config: CloudLlmConfig) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
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
            let body = response.text().unwrap_or_default();
            return Err(format!(
                "HTTP {}: {}",
                status.as_u16(),
                truncate(&body, 512)
            ));
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

    fn build_request_body(&self, context: &StructuredContext) -> Result<Value, String> {
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
        let mut request = json!({
            "model": self.config.model,
            "temperature": self.config.temperature,
            "response_format": ChatResponseFormat {
                kind: "json_object".to_string(),
            },
            "messages": messages,
        });

        if let Some(reasoning_effort) = &self.config.reasoning_effort {
            insert_json_field(
                &mut request,
                "reasoning_effort",
                Value::String(reasoning_effort.clone()),
            );
        }

        match self.config.provider {
            CloudProvider::DeepSeek => {
                if let Some(enable_thinking) = self.config.enable_thinking {
                    insert_json_field(
                        &mut request,
                        "thinking",
                        json!({
                            "type": if enable_thinking { "enabled" } else { "disabled" }
                        }),
                    );
                }
            },
            CloudProvider::Qwen => {
                if let Some(enable_thinking) = self.config.enable_thinking {
                    insert_json_field(
                        &mut request,
                        "enable_thinking",
                        Value::Bool(enable_thinking),
                    );
                }
            },
            CloudProvider::GenericOpenAiCompatible => {},
        }

        Ok(request)
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

#[derive(Debug, Clone)]
struct CloudLlmConfig {
    provider: CloudProvider,
    endpoint: String,
    model: String,
    api_key: Option<String>,
    timeout_secs: u64,
    temperature: f32,
    system_prompt: String,
    reasoning_effort: Option<String>,
    enable_thinking: Option<bool>,
}

impl CloudLlmConfig {
    fn from_env() -> Result<Self, String> {
        let provider = read_var("DIPECS_CLOUD_LLM_PROVIDER")
            .as_deref()
            .map(CloudProvider::parse)
            .transpose()?
            .unwrap_or(CloudProvider::DeepSeek);

        let endpoint = read_var("DIPECS_CLOUD_LLM_ENDPOINT")
            .unwrap_or_else(|| provider.default_endpoint().to_string());
        if endpoint.is_empty() {
            return Err(
                "DIPECS_CLOUD_LLM_ENDPOINT is required when cloud LLM is enabled".to_string(),
            );
        }

        let model = read_var("DIPECS_CLOUD_LLM_MODEL")
            .unwrap_or_else(|| provider.default_model().to_string());
        if model.is_empty() {
            return Err("DIPECS_CLOUD_LLM_MODEL is required when cloud LLM is enabled".to_string());
        }

        Ok(Self {
            provider,
            endpoint,
            model,
            api_key: provider
                .api_key_candidates()
                .iter()
                .find_map(|key| read_var(key)),
            timeout_secs: read_u64_var("DIPECS_CLOUD_LLM_TIMEOUT_SECS")
                .unwrap_or(DEFAULT_TIMEOUT_SECS),
            temperature: read_f32_var("DIPECS_CLOUD_LLM_TEMPERATURE")
                .unwrap_or(DEFAULT_TEMPERATURE),
            system_prompt: read_var("DIPECS_CLOUD_LLM_SYSTEM_PROMPT")
                .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string()),
            reasoning_effort: read_var("DIPECS_CLOUD_LLM_REASONING_EFFORT"),
            enable_thinking: read_bool_var("DIPECS_CLOUD_LLM_ENABLE_THINKING"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloudProvider {
    GenericOpenAiCompatible,
    DeepSeek,
    Qwen,
}

impl CloudProvider {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "generic" | "openai-compatible" | "openai_compatible" | "openai" => {
                Ok(Self::GenericOpenAiCompatible)
            },
            "deepseek" => Ok(Self::DeepSeek),
            "qwen" | "dashscope" => Ok(Self::Qwen),
            _ => Err(format!(
                "unsupported DIPECS_CLOUD_LLM_PROVIDER: {raw} (expected generic, deepseek, or qwen)"
            )),
        }
    }

    fn default_endpoint(self) -> &'static str {
        match self {
            Self::GenericOpenAiCompatible => "",
            Self::DeepSeek => DEFAULT_DEEPSEEK_ENDPOINT,
            Self::Qwen => DEFAULT_QWEN_ENDPOINT,
        }
    }

    fn default_model(self) -> &'static str {
        match self {
            Self::GenericOpenAiCompatible => "",
            Self::DeepSeek => "deepseek-v4-flash",
            Self::Qwen => "qwen-plus",
        }
    }

    fn api_key_candidates(self) -> &'static [&'static str] {
        match self {
            Self::GenericOpenAiCompatible => &["DIPECS_CLOUD_LLM_API_KEY"],
            Self::DeepSeek => &["DIPECS_CLOUD_LLM_API_KEY", "DEEPSEEK_API_KEY"],
            Self::Qwen => &["DIPECS_CLOUD_LLM_API_KEY", "DASHSCOPE_API_KEY"],
        }
    }
}

fn read_var(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.is_empty())
}

fn read_bool_var(key: &str) -> Option<bool> {
    read_var(key).and_then(|value| parse_bool(&value))
}

fn read_u64_var(key: &str) -> Option<u64> {
    read_var(key).and_then(|value| value.parse().ok())
}

fn read_f32_var(key: &str) -> Option<f32> {
    read_var(key).and_then(|value| value.parse().ok())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn insert_json_field(target: &mut Value, key: &str, value: Value) {
    if let Some(object) = target.as_object_mut() {
        object.insert(key.to_string(), value);
    }
}

fn translate_intents(intents: Vec<ModelIntent>) -> Result<Vec<Intent>, String> {
    if intents.is_empty() {
        return Ok(vec![idle_intent()]);
    }

    intents.into_iter().map(translate_intent).collect()
}

fn translate_intent(intent: ModelIntent) -> Result<Intent, String> {
    let prefetch_category = infer_prefetch_category(&intent);
    let prefetched_target = infer_prefetch_target(&intent, prefetch_category.as_ref());
    let intent_type = parse_intent_type(
        &intent.intent_type,
        intent.target.clone(),
        intent.extension_category.as_deref(),
    )?;
    let suggested_actions = if intent.actions.is_empty() {
        vec![SuggestedAction {
            action_type: ActionType::NoOp,
            target: None,
            urgency: ActionUrgency::IdleTime,
        }]
    } else {
        intent
            .actions
            .into_iter()
            .map(|action| {
                translate_action(
                    action,
                    prefetched_target.as_deref(),
                    prefetch_category.as_ref(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(Intent {
        intent_id: new_id(),
        intent_type,
        confidence: intent.confidence.clamp(0.0, 1.0),
        risk_level: parse_risk_level(&intent.risk_level)?,
        suggested_actions,
        rationale_tags: if intent.rationale_tags.is_empty() {
            vec!["cloud_llm".into()]
        } else {
            intent.rationale_tags
        },
    })
}

fn translate_action(
    action: ModelAction,
    fallback_prefetch_target: Option<&str>,
    prefetch_category: Option<&ExtensionCategory>,
) -> Result<SuggestedAction, String> {
    let action_type = parse_action_type(&action.action_type)?;
    let target = match action_type {
        ActionType::PrefetchFile => normalize_prefetch_target(
            action.target.filter(|value| !value.trim().is_empty()),
            fallback_prefetch_target,
            prefetch_category,
        ),
        _ => action.target.filter(|value| !value.trim().is_empty()),
    };
    Ok(SuggestedAction {
        action_type,
        target,
        urgency: action
            .urgency
            .as_deref()
            .map(parse_action_urgency)
            .transpose()?
            .unwrap_or(ActionUrgency::IdleTime),
    })
}

fn parse_intent_type(
    raw: &str,
    target: Option<String>,
    extension_category: Option<&str>,
) -> Result<IntentType, String> {
    match normalize_enum_name(raw).as_str() {
        "openapp" => Ok(IntentType::OpenApp(target.unwrap_or_default())),
        "switchtoapp" => Ok(IntentType::SwitchToApp(target.unwrap_or_default())),
        "checknotification" => Ok(IntentType::CheckNotification(target.unwrap_or_default())),
        "handlefile" => Ok(IntentType::HandleFile(parse_extension_category(
            extension_category.unwrap_or("Unknown"),
        )?)),
        "entercontext" => Ok(IntentType::EnterContext(target.unwrap_or_default())),
        "idle" => Ok(IntentType::Idle),
        _ => Err(format!("unsupported intent_type: {raw}")),
    }
}

fn parse_risk_level(raw: &str) -> Result<RiskLevel, String> {
    match normalize_enum_name(raw).as_str() {
        "low" => Ok(RiskLevel::Low),
        "medium" => Ok(RiskLevel::Medium),
        "high" => Ok(RiskLevel::High),
        _ => Err(format!("unsupported risk_level: {raw}")),
    }
}

fn parse_action_type(raw: &str) -> Result<ActionType, String> {
    match normalize_enum_name(raw).as_str() {
        "prewarmprocess" => Ok(ActionType::PreWarmProcess),
        "prefetchfile" => Ok(ActionType::PrefetchFile),
        "keepalive" => Ok(ActionType::KeepAlive),
        "releasememory" => Ok(ActionType::ReleaseMemory),
        "noop" => Ok(ActionType::NoOp),
        _ => Err(format!("unsupported action_type: {raw}")),
    }
}

fn parse_action_urgency(raw: &str) -> Result<ActionUrgency, String> {
    match normalize_enum_name(raw).as_str() {
        "immediate" => Ok(ActionUrgency::Immediate),
        "idletime" | "idle" => Ok(ActionUrgency::IdleTime),
        "deferred" => Ok(ActionUrgency::Deferred),
        _ => Err(format!("unsupported urgency: {raw}")),
    }
}

fn parse_extension_category(raw: &str) -> Result<ExtensionCategory, String> {
    match normalize_enum_name(raw).as_str() {
        "document" => Ok(ExtensionCategory::Document),
        "image" => Ok(ExtensionCategory::Image),
        "video" => Ok(ExtensionCategory::Video),
        "audio" => Ok(ExtensionCategory::Audio),
        "archive" => Ok(ExtensionCategory::Archive),
        "code" => Ok(ExtensionCategory::Code),
        "other" => Ok(ExtensionCategory::Other),
        "unknown" => Ok(ExtensionCategory::Unknown),
        _ => Err(format!("unsupported extension_category: {raw}")),
    }
}

fn infer_prefetch_category(intent: &ModelIntent) -> Option<ExtensionCategory> {
    if normalize_enum_name(&intent.intent_type) != "handlefile" {
        return None;
    }

    Some(
        intent
            .extension_category
            .as_deref()
            .and_then(|raw| parse_extension_category(raw).ok())
            .unwrap_or(ExtensionCategory::Unknown),
    )
}

fn infer_prefetch_target(
    intent: &ModelIntent,
    extension_category: Option<&ExtensionCategory>,
) -> Option<String> {
    if normalize_enum_name(&intent.intent_type) != "handlefile" {
        return None;
    }

    let category = extension_category
        .cloned()
        .unwrap_or(ExtensionCategory::Unknown);
    Some(default_prefetch_target(&category, intent.target.as_deref()))
}

fn normalize_prefetch_target(
    target: Option<String>,
    fallback: Option<&str>,
    prefetch_category: Option<&ExtensionCategory>,
) -> Option<String> {
    let normalized = target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| {
            if value.starts_with("url:") || value.starts_with("uri:") {
                Some(value.to_string())
            } else if value.starts_with("http://") || value.starts_with("https://") {
                Some(format!("url:{value}"))
            } else if value.starts_with("content://") {
                Some(format!("uri:{value}"))
            } else if let Some(package_name) = value.strip_prefix("pkg:") {
                prefetch_category
                    .map(|category| default_prefetch_target(category, Some(package_name.trim())))
            } else if looks_like_package_name(value) {
                prefetch_category.map(|category| default_prefetch_target(category, Some(value)))
            } else {
                None
            }
        });

    normalized.or_else(|| fallback.map(str::to_string))
}

fn normalize_enum_name(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn parse_model_output(content: &str) -> Result<ModelOutput, String> {
    let stripped = strip_code_fences(content);
    let cleaned = stripped.trim();
    serde_json::from_str(cleaned)
        .map_err(|error| format!("model output was not valid JSON: {error}"))
}

fn strip_code_fences(content: &str) -> String {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let without_prefix = trimmed
        .split_once('\n')
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    without_prefix
        .strip_suffix("```")
        .map(str::trim)
        .unwrap_or(without_prefix)
        .to_string()
}

fn idle_batch(context: &StructuredContext, model: String) -> IntentBatch {
    IntentBatch {
        window_id: context.window_id.clone(),
        intents: vec![idle_intent()],
        generated_at_ms: context.window_end_ms,
        model,
    }
}

fn idle_intent() -> Intent {
    Intent {
        intent_id: new_id(),
        intent_type: IntentType::Idle,
        confidence: 0.5,
        risk_level: RiskLevel::Low,
        suggested_actions: vec![SuggestedAction {
            action_type: ActionType::NoOp,
            target: None,
            urgency: ActionUrgency::IdleTime,
        }],
        rationale_tags: vec!["cloud_llm_idle_fallback".into()],
    }
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
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

#[derive(Debug, Deserialize)]
struct ModelOutput {
    intents: Vec<ModelIntent>,
}

#[derive(Debug, Deserialize)]
struct ModelIntent {
    intent_type: String,
    target: Option<String>,
    extension_category: Option<String>,
    confidence: f32,
    risk_level: String,
    #[serde(default)]
    actions: Vec<ModelAction>,
    #[serde(default)]
    rationale_tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ModelAction {
    action_type: String,
    target: Option<String>,
    urgency: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        infer_prefetch_category, infer_prefetch_target, normalize_prefetch_target, parse_bool,
        translate_action, CloudProvider, ModelAction, ModelIntent,
    };
    use crate::backends::prefetch_target::default_prefetch_target;
    use aios_spec::{ActionType, ExtensionCategory};

    #[test]
    fn provider_parser_accepts_known_values() {
        assert_eq!(
            CloudProvider::parse("deepseek").unwrap(),
            CloudProvider::DeepSeek
        );
        assert_eq!(CloudProvider::parse("qwen").unwrap(), CloudProvider::Qwen);
        assert_eq!(
            CloudProvider::parse("openai-compatible").unwrap(),
            CloudProvider::GenericOpenAiCompatible
        );
    }

    #[test]
    fn bool_parser_accepts_common_values() {
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool("0"), Some(false));
    }

    #[test]
    fn normalize_prefetch_target_adds_url_prefix() {
        let target =
            normalize_prefetch_target(Some("https://example.test/feed.json".into()), None, None);
        assert_eq!(
            target.as_deref(),
            Some("url:https://example.test/feed.json")
        );
    }

    #[test]
    fn normalize_prefetch_target_adds_uri_prefix() {
        let target =
            normalize_prefetch_target(Some("content://downloads/document/1".into()), None, None);
        assert_eq!(
            target.as_deref(),
            Some("uri:content://downloads/document/1")
        );
    }

    #[test]
    fn normalize_prefetch_target_resolves_pkg_target() {
        let target = normalize_prefetch_target(
            Some("pkg:com.ss.android.lark".into()),
            None,
            Some(&ExtensionCategory::Document),
        );
        assert_eq!(target.as_deref(), Some("url:https://www.feishu.cn/docx/"));
    }

    #[test]
    fn translate_action_uses_fallback_prefetch_target() {
        let action = translate_action(
            ModelAction {
                action_type: "PrefetchFile".into(),
                target: None,
                urgency: Some("IdleTime".into()),
            },
            Some("url:https://www.feishu.cn/docx/"),
            Some(&ExtensionCategory::Document),
        )
        .unwrap();

        assert!(matches!(action.action_type, ActionType::PrefetchFile));
        assert_eq!(
            action.target.as_deref(),
            Some("url:https://www.feishu.cn/docx/")
        );
    }

    #[test]
    fn infer_prefetch_target_for_handle_file_uses_extension_category() {
        let intent = ModelIntent {
            intent_type: "HandleFile".into(),
            target: Some("com.example.files".into()),
            extension_category: Some("Document".into()),
            confidence: 0.8,
            risk_level: "Low".into(),
            actions: vec![],
            rationale_tags: vec![],
        };

        let category = infer_prefetch_category(&intent).unwrap();
        let target = infer_prefetch_target(&intent, Some(&category)).unwrap();
        assert_eq!(
            target,
            default_prefetch_target(&ExtensionCategory::Document, Some("com.example.files"))
        );
    }
}
