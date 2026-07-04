use std::env;

const ENV_ENABLED: &str = "DIPECS_CLOUD_LLM_ENABLED";
const ENV_PROVIDER: &str = "DIPECS_CLOUD_LLM_PROVIDER";
const ENV_ENDPOINT: &str = "DIPECS_CLOUD_LLM_ENDPOINT";
const ENV_MODEL: &str = "DIPECS_CLOUD_LLM_MODEL";
const ENV_API_KEY: &str = "DIPECS_CLOUD_LLM_API_KEY";
const ENV_TIMEOUT_SECS: &str = "DIPECS_CLOUD_LLM_TIMEOUT_SECS";
const ENV_TEMPERATURE: &str = "DIPECS_CLOUD_LLM_TEMPERATURE";
const ENV_SYSTEM_PROMPT: &str = "DIPECS_CLOUD_LLM_SYSTEM_PROMPT";
const ENV_REASONING_EFFORT: &str = "DIPECS_CLOUD_LLM_REASONING_EFFORT";
const ENV_ENABLE_THINKING: &str = "DIPECS_CLOUD_LLM_ENABLE_THINKING";
const ENV_DEEPSEEK_API_KEY: &str = "DEEPSEEK_API_KEY";
const ENV_DASHSCOPE_API_KEY: &str = "DASHSCOPE_API_KEY";

const DEFAULT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_TEMPERATURE: f32 = 0.1;
const DEFAULT_DEEPSEEK_ENDPOINT: &str = "https://api.deepseek.com/chat/completions";
const DEFAULT_QWEN_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions";
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are the decision backend for DiPECS.
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
- Intent type selection:
  * HandleFile: when file_activity is present or notification references a file.
  * CheckNotification: for notifications (especially with semantic_hints like
    LinkAttachment, FinancialContext, VerificationCode, FileMention).
  * OpenApp/SwitchToApp: for foreground app transitions.
  * Idle: when no actionable signal exists.
- Action selection based on semantic hints and context:
  * PreWarmProcess when notified_apps have semantic_hints (LinkAttachment,
    FinancialContext, VerificationCode, FileMention) — user will likely
    switch to that app soon.
  * PrefetchFile when file_activity is present with a specific extension.
  * ReleaseMemory when battery is low and not charging.
  * KeepAlive when charging or when system needs sustained background work.
  * NoOp only when there is genuinely no actionable signal.
- For PrefetchFile targets, use the app package name (e.g. `com.example.chat`)
  or a concrete Android bridge target (`url:https://...`, `uri:content://...`).
- For PreWarmProcess, use `own:resources` or `pkg:<observed.package>`.
- For KeepAlive, use `work:collector_heartbeat`.
- For ReleaseMemory, use `cache:prefetch`.
- Use short snake_case rationale tags.
- The user message contains `model_input_json` with:
  - `current_context`: the current sanitized window (events, summary).
  - `behavior_profile`: long-running privacy-preserving habit summary.
  - `recent_feedback`: recent decisions plus local policy/execution outcomes.
- Prefer current_context for immediate facts, use behavior_profile for stable
  tendencies, and use recent_feedback to avoid repeating recently denied,
  failed, or low-value actions.
"#;

pub(super) fn cloud_llm_enabled() -> bool {
    read_bool_var(ENV_ENABLED).unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct CloudLlmConfig {
    pub provider: CloudProvider,
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout_secs: u64,
    pub temperature: f32,
    pub system_prompt: String,
    pub reasoning_effort: Option<String>,
    pub enable_thinking: Option<bool>,
}

impl CloudLlmConfig {
    pub fn from_env() -> Result<Self, String> {
        Self::from_env_inner()
    }

    fn from_env_inner() -> Result<Self, String> {
        let provider = read_var(ENV_PROVIDER)
            .as_deref()
            .map(CloudProvider::parse)
            .transpose()?
            .unwrap_or(CloudProvider::DeepSeek);

        let endpoint =
            read_var(ENV_ENDPOINT).unwrap_or_else(|| provider.default_endpoint().to_string());
        if endpoint.is_empty() {
            return Err(format!(
                "{ENV_ENDPOINT} is required when cloud LLM is enabled"
            ));
        }

        let model = read_var(ENV_MODEL).unwrap_or_else(|| provider.default_model().to_string());
        if model.is_empty() {
            return Err(format!("{ENV_MODEL} is required when cloud LLM is enabled"));
        }

        Ok(Self {
            provider,
            endpoint,
            model,
            api_key: provider
                .api_key_candidates()
                .iter()
                .find_map(|key| read_var(key)),
            timeout_secs: read_u64_var(ENV_TIMEOUT_SECS).unwrap_or(DEFAULT_TIMEOUT_SECS),
            temperature: read_f32_var(ENV_TEMPERATURE).unwrap_or(DEFAULT_TEMPERATURE),
            system_prompt: read_var(ENV_SYSTEM_PROMPT)
                .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string()),
            reasoning_effort: read_var(ENV_REASONING_EFFORT),
            enable_thinking: read_bool_var(ENV_ENABLE_THINKING),
        })
    }

    /// Test/baseline-only constructor for latency/benchmark tests against a real provider.
    #[cfg(test)]
    pub(crate) fn new_for_test(
        provider: CloudProvider,
        endpoint: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            endpoint: endpoint.into(),
            model: model.into(),
            api_key: Some(api_key.into()),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            temperature: DEFAULT_TEMPERATURE,
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
            reasoning_effort: None,
            enable_thinking: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudProvider {
    GenericOpenAiCompatible,
    DeepSeek,
    Qwen,
}

impl CloudProvider {
    pub(super) fn parse(raw: &str) -> Result<Self, String> {
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
            Self::GenericOpenAiCompatible => &[ENV_API_KEY],
            Self::DeepSeek => &[ENV_API_KEY, ENV_DEEPSEEK_API_KEY],
            Self::Qwen => &[ENV_API_KEY, ENV_DASHSCOPE_API_KEY],
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

#[cfg(test)]
mod tests {
    use super::{parse_bool, CloudProvider};

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
}
