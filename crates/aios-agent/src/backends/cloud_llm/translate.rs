use aios_spec::{
    ActionType, ActionUrgency, ExtensionCategory, Intent, IntentBatch, IntentType, RiskLevel,
    StructuredContext, SuggestedAction,
};
use serde::Deserialize;

use crate::backends::prefetch_target::{default_prefetch_target, looks_like_package_name};
use crate::new_id;

pub(super) fn translate_intents(intents: Vec<ModelIntent>) -> Result<Vec<Intent>, String> {
    if intents.is_empty() {
        return Ok(vec![idle_intent()]);
    }

    intents.into_iter().map(translate_intent).collect()
}

pub(super) fn parse_model_output(content: &str) -> Result<ModelOutput, String> {
    let stripped = strip_code_fences(content);
    let cleaned = stripped.trim();
    serde_json::from_str(cleaned)
        .map_err(|error| format!("model output was not valid JSON: {error}"))
}

pub(super) fn idle_batch(context: &StructuredContext, model: String) -> IntentBatch {
    IntentBatch {
        window_id: context.window_id.clone(),
        intents: vec![idle_intent()],
        generated_at_ms: context.window_end_ms,
        model,
    }
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

#[derive(Debug, Deserialize)]
pub(super) struct ModelOutput {
    pub(super) intents: Vec<ModelIntent>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ModelIntent {
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
        infer_prefetch_category, infer_prefetch_target, normalize_prefetch_target,
        parse_model_output, translate_action, translate_intents, ModelAction, ModelIntent,
    };
    use crate::backends::prefetch_target::default_prefetch_target;
    use aios_spec::{ActionType, ExtensionCategory, IntentType};

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
    fn parse_model_output_rejects_invalid_json() {
        let err = parse_model_output("not json").unwrap_err();
        assert!(err.contains("valid JSON"), "got: {err}");
    }

    #[test]
    fn parse_model_output_rejects_empty_content() {
        let err = parse_model_output("   ").unwrap_err();
        assert!(err.contains("valid JSON"), "got: {err}");
    }

    #[test]
    fn translate_intents_empty_returns_idle() {
        let intents = translate_intents(vec![]).unwrap();
        assert_eq!(intents.len(), 1);
        assert!(matches!(intents[0].intent_type, IntentType::Idle));
        assert!(matches!(
            intents[0].suggested_actions[0].action_type,
            ActionType::NoOp
        ));
    }

    #[test]
    fn translate_intents_rejects_unknown_intent_type() {
        let out = parse_model_output(r#"{"intents":[{"intent_type":"UnknownIntent","target":null,"confidence":0.8,"risk_level":"Low","actions":[]}]}"#).unwrap();
        let err = translate_intents(out.intents).unwrap_err();
        assert!(err.contains("unsupported intent_type"), "got: {err}");
    }

    #[test]
    fn translate_intents_rejects_unknown_action_type() {
        let out = parse_model_output(r#"{"intents":[{"intent_type":"Idle","target":null,"confidence":0.8,"risk_level":"Low","actions":[{"action_type":"ExplodeDevice","target":null,"urgency":null}]}]}"#).unwrap();
        let err = translate_intents(out.intents).unwrap_err();
        assert!(err.contains("unsupported action_type"), "got: {err}");
    }

    #[test]
    fn translate_intents_rejects_unknown_risk_level() {
        let out = parse_model_output(r#"{"intents":[{"intent_type":"Idle","target":null,"confidence":0.8,"risk_level":"Critical","actions":[]}]}"#).unwrap();
        let err = translate_intents(out.intents).unwrap_err();
        assert!(err.contains("unsupported risk_level"), "got: {err}");
    }

    #[test]
    fn translate_intents_clamps_confidence() {
        let out = parse_model_output(r#"{"intents":[{"intent_type":"Idle","target":null,"confidence":1.5,"risk_level":"Low","actions":[]}]}"#).unwrap();
        let intents = translate_intents(out.intents).unwrap();
        assert_eq!(intents[0].confidence, 1.0);

        let out = parse_model_output(r#"{"intents":[{"intent_type":"Idle","target":null,"confidence":-0.3,"risk_level":"Low","actions":[]}]}"#).unwrap();
        let intents = translate_intents(out.intents).unwrap();
        assert_eq!(intents[0].confidence, 0.0);
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

    #[test]
    fn translate_intents_rejects_unknown_urgency() {
        let out = parse_model_output(r#"{"intents":[{"intent_type":"Idle","target":null,"confidence":0.8,"risk_level":"Low","actions":[{"action_type":"NoOp","target":null,"urgency":"Sometime"}]}]}"#).unwrap();
        let err = translate_intents(out.intents).unwrap_err();
        assert!(err.contains("unsupported urgency"), "got: {err}");
    }
}
