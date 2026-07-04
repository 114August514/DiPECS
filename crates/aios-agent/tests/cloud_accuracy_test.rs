//! Live DeepSeek accuracy evaluation over labeled sanitized-context cases.
//!
//! Run manually; this intentionally stays ignored because it calls a paid/live
//! provider and needs enough rounds for a stable estimate:
//!
//! ```bash
//! DIPECS_CLOUD_LLM_API_KEY=sk-xxx CLOUD_ACCURACY_ROUNDS=3 \
//! cargo test -p aios-agent --test cloud_accuracy_test -- --ignored --nocapture
//! ```

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use aios_agent::{CloudLlmBackend, CloudLlmConfig, DecisionBackend};
use aios_spec::{
    ActionType, AppTransition, ContextSummary, ExtensionCategory, FsActivityType, Intent,
    IntentType, LocationType, NetworkType, RingerMode, SanitizedEvent, SanitizedEventType,
    ScriptHint, SemanticHint, SourceTier, StructuredContext, SuggestedAction, SystemStatusSnapshot,
    TextHint,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AccuracyDataset {
    cases: Vec<AccuracyCase>,
}

#[derive(Debug, Deserialize)]
struct AccuracyCase {
    id: String,
    persona: String,
    scenario: String,
    foreground_apps: Vec<String>,
    notified_apps: Vec<String>,
    semantic_hints: Vec<String>,
    #[serde(default)]
    file_activity: Vec<(String, u32)>,
    system_status: Option<AccuracySystemStatus>,
    expected: Vec<ExpectedDecision>,
}

#[derive(Debug, Deserialize)]
struct AccuracySystemStatus {
    battery_pct: Option<u8>,
    is_charging: bool,
    network: String,
    ringer_mode: String,
    location_type: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedDecision {
    intent_type: String,
    target: Option<String>,
    extension_category: Option<String>,
    action_type: String,
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolve project root")
}

fn parse_semantic_hint(raw: &str) -> SemanticHint {
    match raw {
        "FileMention" => SemanticHint::FileMention,
        "ImageMention" => SemanticHint::ImageMention,
        "AudioMessage" => SemanticHint::AudioMessage,
        "LinkAttachment" => SemanticHint::LinkAttachment,
        "UserMentioned" => SemanticHint::UserMentioned,
        "CalendarInvitation" => SemanticHint::CalendarInvitation,
        "FinancialContext" => SemanticHint::FinancialContext,
        "VerificationCode" => SemanticHint::VerificationCode,
        other => panic!("unsupported semantic hint: {other}"),
    }
}

fn parse_extension_category(raw: &str) -> ExtensionCategory {
    match raw {
        "Document" => ExtensionCategory::Document,
        "Image" => ExtensionCategory::Image,
        "Video" => ExtensionCategory::Video,
        "Audio" => ExtensionCategory::Audio,
        "Archive" => ExtensionCategory::Archive,
        "Code" => ExtensionCategory::Code,
        "Other" => ExtensionCategory::Other,
        "Unknown" => ExtensionCategory::Unknown,
        other => panic!("unsupported extension category: {other}"),
    }
}

fn parse_network(raw: &str) -> NetworkType {
    match raw {
        "Wifi" => NetworkType::Wifi,
        "Cellular" => NetworkType::Cellular,
        "Offline" => NetworkType::Offline,
        "Unknown" => NetworkType::Unknown,
        other => panic!("unsupported network: {other}"),
    }
}

fn parse_ringer(raw: &str) -> RingerMode {
    match raw {
        "Normal" => RingerMode::Normal,
        "Vibrate" => RingerMode::Vibrate,
        "Silent" => RingerMode::Silent,
        other => panic!("unsupported ringer mode: {other}"),
    }
}

fn parse_location(raw: &str) -> LocationType {
    match raw {
        "Home" => LocationType::Home,
        "Work" => LocationType::Work,
        "Commute" => LocationType::Commute,
        "Unknown" => LocationType::Unknown,
        other => panic!("unsupported location type: {other}"),
    }
}

fn extension_name(category: &ExtensionCategory) -> &'static str {
    match category {
        ExtensionCategory::Document => "Document",
        ExtensionCategory::Image => "Image",
        ExtensionCategory::Video => "Video",
        ExtensionCategory::Audio => "Audio",
        ExtensionCategory::Archive => "Archive",
        ExtensionCategory::Code => "Code",
        ExtensionCategory::Other => "Other",
        ExtensionCategory::Unknown => "Unknown",
    }
}

fn action_name(action: &ActionType) -> &'static str {
    match action {
        ActionType::PreWarmProcess => "PreWarmProcess",
        ActionType::PrefetchFile => "PrefetchFile",
        ActionType::KeepAlive => "KeepAlive",
        ActionType::ReleaseMemory => "ReleaseMemory",
        ActionType::NoOp => "NoOp",
    }
}

fn load_accuracy_cases() -> AccuracyDataset {
    let path = env::var("CLOUD_ACCURACY_CASES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| project_root().join("data/evaluation/cloud-llm-accuracy-cases.json"));
    let file = fs::File::open(&path).unwrap_or_else(|_| panic!("open {}", path.display()));
    serde_json::from_reader(file).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

fn context_from_case(case: &AccuracyCase) -> StructuredContext {
    let mut events = Vec::new();
    let start = 1_718_000_000_000i64;
    let hints: Vec<SemanticHint> = case
        .semantic_hints
        .iter()
        .map(|hint| parse_semantic_hint(hint))
        .collect();

    for (idx, app) in case.foreground_apps.iter().enumerate() {
        events.push(SanitizedEvent {
            event_id: format!("{}-fg-{idx}", case.id),
            timestamp_ms: start + idx as i64 * 1000,
            event_type: SanitizedEventType::AppTransition {
                package_name: app.clone(),
                activity_class: None,
                transition: AppTransition::Foreground,
            },
            source_tier: SourceTier::PublicApi,
            app_package: Some(app.clone()),
            uid: None,
        });
    }

    for (idx, app) in case.notified_apps.iter().enumerate() {
        events.push(SanitizedEvent {
            event_id: format!("{}-notif-{idx}", case.id),
            timestamp_ms: start + 10_000 + idx as i64 * 1000,
            event_type: SanitizedEventType::Notification {
                source_package: app.clone(),
                category: Some("msg".into()),
                channel_id: None,
                title_hint: TextHint {
                    length_chars: 24,
                    script: ScriptHint::Latin,
                    is_emoji_only: false,
                },
                text_hint: TextHint {
                    length_chars: 80,
                    script: ScriptHint::Latin,
                    is_emoji_only: false,
                },
                semantic_hints: hints.clone(),
                is_ongoing: false,
                group_key: None,
            },
            source_tier: SourceTier::PublicApi,
            app_package: Some(app.clone()),
            uid: None,
        });
    }

    for (idx, (category, _count)) in case.file_activity.iter().enumerate() {
        let package_name = case
            .notified_apps
            .first()
            .or_else(|| case.foreground_apps.first())
            .cloned();
        events.push(SanitizedEvent {
            event_id: format!("{}-file-{idx}", case.id),
            timestamp_ms: start + 20_000 + idx as i64 * 1000,
            event_type: SanitizedEventType::FileActivity {
                package_name: package_name.clone(),
                extension_category: parse_extension_category(category),
                activity_type: FsActivityType::Read,
                is_hot_file: true,
            },
            source_tier: SourceTier::PublicApi,
            app_package: package_name,
            uid: None,
        });
    }

    let latest_system_status = case
        .system_status
        .as_ref()
        .map(|status| SystemStatusSnapshot {
            battery_pct: status.battery_pct,
            is_charging: status.is_charging,
            network: parse_network(&status.network),
            ringer_mode: parse_ringer(&status.ringer_mode),
            location_type: parse_location(&status.location_type),
            headphone_connected: false,
        });

    if let Some(status) = &latest_system_status {
        events.push(SanitizedEvent {
            event_id: format!("{}-system", case.id),
            timestamp_ms: start + 30_000,
            event_type: SanitizedEventType::SystemStatus {
                battery_pct: status.battery_pct,
                is_charging: status.is_charging,
                network: status.network.clone(),
                ringer_mode: status.ringer_mode.clone(),
                location_type: status.location_type.clone(),
                headphone_connected: status.headphone_connected,
            },
            source_tier: SourceTier::PublicApi,
            app_package: None,
            uid: None,
        });
    }

    StructuredContext {
        window_id: case.id.clone(),
        window_start_ms: start,
        window_end_ms: start + 60_000,
        duration_secs: 60,
        events,
        summary: ContextSummary {
            foreground_apps: case.foreground_apps.clone(),
            notified_apps: case.notified_apps.clone(),
            all_semantic_hints: hints,
            file_activity: case
                .file_activity
                .iter()
                .map(|(category, count)| (parse_extension_category(category), *count))
                .collect(),
            latest_system_status,
            source_tier: SourceTier::PublicApi,
        },
    }
}

fn intent_parts(intent: &Intent) -> (&'static str, Option<String>, Option<&'static str>) {
    match &intent.intent_type {
        IntentType::OpenApp(target) => ("OpenApp", Some(target.clone()), None),
        IntentType::SwitchToApp(target) => ("SwitchToApp", Some(target.clone()), None),
        IntentType::CheckNotification(target) => ("CheckNotification", Some(target.clone()), None),
        IntentType::HandleFile(category) => ("HandleFile", None, Some(extension_name(category))),
        IntentType::EnterContext(target) => ("EnterContext", Some(target.clone()), None),
        IntentType::Idle => ("Idle", None, None),
    }
}

fn matches_expected(
    intent: &Intent,
    action: &SuggestedAction,
    expected: &ExpectedDecision,
) -> bool {
    let (intent_name, target, extension_category) = intent_parts(intent);
    if intent_name != expected.intent_type {
        return false;
    }
    if action_name(&action.action_type) != expected.action_type {
        return false;
    }
    if let Some(expected_target) = &expected.target {
        let observed_target = target.as_ref().or(action.target.as_ref());
        if observed_target != Some(expected_target) {
            return false;
        }
    }
    if let Some(expected_category) = &expected.extension_category {
        if extension_category != Some(expected_category.as_str()) {
            return false;
        }
    }
    true
}

fn hit_rank(result: &aios_spec::DecisionBackendResult, case: &AccuracyCase) -> Option<usize> {
    let mut rank = 0usize;
    for intent in &result.intent_batch.intents {
        for action in &intent.suggested_actions {
            rank += 1;
            if case
                .expected
                .iter()
                .any(|expected| matches_expected(intent, action, expected))
            {
                return Some(rank);
            }
        }
    }
    None
}

fn rendered_intents(result: &aios_spec::DecisionBackendResult) -> Vec<String> {
    result
        .intent_batch
        .intents
        .iter()
        .flat_map(|intent| {
            let (intent_name, target, extension_category) = intent_parts(intent);
            intent.suggested_actions.iter().map(move |action| {
                format!(
                    "{intent_name}(target={:?}, category={:?}) -> {}({:?})",
                    target,
                    extension_category,
                    action_name(&action.action_type),
                    action.target
                )
            })
        })
        .collect()
}

fn now_ts() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[test]
#[ignore = "requires DIPECS_CLOUD_LLM_API_KEY and calls the live DeepSeek API"]
fn deepseek_suggestion_accuracy_over_labeled_personas() {
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        panic!("rustls ring provider install failed: {e:?}");
    }

    // Ensure API key is available (from_env() reads it internally)
    assert!(
        env::var("DIPECS_CLOUD_LLM_API_KEY")
            .or_else(|_| env::var("DEEPSEEK_API_KEY"))
            .is_ok(),
        "set DIPECS_CLOUD_LLM_API_KEY or DEEPSEEK_API_KEY"
    );
    let rounds = env::var("CLOUD_ACCURACY_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3usize);
    let min_accuracy = env::var("CLOUD_ACCURACY_MIN_PCT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(90.0f64);

    let dataset = load_accuracy_cases();
    assert!(dataset.cases.len() >= 30);

    // Set env vars so from_env() picks up the correct values
    env::set_var(
        "DIPECS_CLOUD_LLM_ENDPOINT",
        env::var("DIPECS_CLOUD_LLM_ENDPOINT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "https://api.deepseek.com/chat/completions".into()),
    );
    env::set_var(
        "DIPECS_CLOUD_LLM_MODEL",
        env::var("DIPECS_CLOUD_LLM_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "deepseek-v4-flash".into()),
    );
    let config = CloudLlmConfig::from_env().expect("cloud config from env failed");
    let model = config.model.clone();
    let backend = CloudLlmBackend::try_new(config).expect("cloud backend init failed");

    let mut hits = 0u32;
    let mut top1_hits = 0u32;
    let mut top3_hits = 0u32;
    let mut top5_hits = 0u32;
    let mut misses = 0u32;
    let mut errors = 0u32;
    let mut latencies = Vec::new();
    let mut case_results = Vec::new();

    println!(
        "\n=== DeepSeek suggestion accuracy: cases={} rounds={} threshold={min_accuracy:.1}% ===",
        dataset.cases.len(),
        rounds
    );

    for case in &dataset.cases {
        let ctx = context_from_case(case);
        let mut case_hits = 0u32;
        let mut case_top1_hits = 0u32;
        let mut case_top3_hits = 0u32;
        let mut case_top5_hits = 0u32;
        let mut case_errors = 0u32;
        let mut observed = Vec::new();

        for round in 0..rounds {
            let start = Instant::now();
            let result = backend.evaluate(&ctx);
            let wall_ms = start.elapsed().as_millis() as u64;
            latencies.push(wall_ms);

            if let Some(error) = &result.error {
                errors += 1;
                case_errors += 1;
                observed.push(format!("round {} ERROR: {}", round + 1, error));
                println!(
                    "  {:<28} round {}/{} {:>5}ms ERROR {}",
                    case.id,
                    round + 1,
                    rounds,
                    wall_ms,
                    error
                );
                continue;
            }

            let rank = hit_rank(&result, case);
            let hit = rank.is_some();
            if let Some(rank) = rank {
                hits += 1;
                case_hits += 1;
                if rank <= 1 {
                    top1_hits += 1;
                    case_top1_hits += 1;
                }
                if rank <= 3 {
                    top3_hits += 1;
                    case_top3_hits += 1;
                }
                if rank <= 5 {
                    top5_hits += 1;
                    case_top5_hits += 1;
                }
            } else {
                misses += 1;
            }
            let intents = rendered_intents(&result);
            observed.push(format!(
                "round {} hit={} rank={:?} {:?}",
                round + 1,
                hit,
                rank,
                intents
            ));
            println!(
                "  {:<28} round {}/{} {:>5}ms hit={} {:?}",
                case.id,
                round + 1,
                rounds,
                wall_ms,
                hit,
                intents
            );
        }

        case_results.push(serde_json::json!({
            "id": case.id,
            "persona": case.persona,
            "scenario": case.scenario,
            "rounds": rounds,
            "hits": case_hits,
            "top1_hits": case_top1_hits,
            "top3_hits": case_top3_hits,
            "top5_hits": case_top5_hits,
            "errors": case_errors,
            "case_accuracy_pct": case_hits as f64 / rounds as f64 * 100.0,
            "case_top1_accuracy_pct": case_top1_hits as f64 / rounds as f64 * 100.0,
            "case_top3_accuracy_pct": case_top3_hits as f64 / rounds as f64 * 100.0,
            "case_top5_accuracy_pct": case_top5_hits as f64 / rounds as f64 * 100.0,
            "observed": observed,
        }));
    }

    let scored = hits + misses;
    let accuracy = if scored == 0 {
        0.0
    } else {
        hits as f64 / scored as f64 * 100.0
    };
    let top1_accuracy = if scored == 0 {
        0.0
    } else {
        top1_hits as f64 / scored as f64 * 100.0
    };
    let top3_accuracy = if scored == 0 {
        0.0
    } else {
        top3_hits as f64 / scored as f64 * 100.0
    };
    let top5_accuracy = if scored == 0 {
        0.0
    } else {
        top5_hits as f64 / scored as f64 * 100.0
    };
    let attempted = hits + misses + errors;
    let success_rate = (hits + misses) as f64 / attempted.max(1) as f64 * 100.0;
    latencies.sort_unstable();
    let p50 = latencies.get(latencies.len() / 2).copied().unwrap_or(0);
    let p95 = if latencies.is_empty() {
        0
    } else {
        latencies[((latencies.len() as f64 * 0.95) as usize).min(latencies.len() - 1)]
    };

    println!("\naccuracy={accuracy:.2}% top1={top1_accuracy:.2}% top3={top3_accuracy:.2}% top5={top5_accuracy:.2}% hits={hits} misses={misses} errors={errors}");
    println!("success_rate={success_rate:.2}% latency_p50={p50}ms latency_p95={p95}ms");

    let out = project_root().join("data/evaluation");
    fs::create_dir_all(&out).ok();
    let path = out.join(format!("cloud-accuracy-{}.json", now_ts()));
    let report = serde_json::json!({
        "schema_version": "dipecs.cloud_accuracy.v1",
        "dataset_id": "cloud-llm-accuracy-cases",
        "status": "measured_live_api",
        "environment": {
            "provider": "deepseek",
            "model": model,
            "rounds_per_case": rounds
        },
        "results": {
            "cases": dataset.cases.len(),
            "scored_rounds": scored,
            "hits": hits,
            "top1_hits": top1_hits,
            "top3_hits": top3_hits,
            "top5_hits": top5_hits,
            "misses": misses,
            "errors": errors,
            "accuracy_pct": accuracy,
            "top1_accuracy_pct": top1_accuracy,
            "top3_accuracy_pct": top3_accuracy,
            "top5_accuracy_pct": top5_accuracy,
            "success_rate_pct": success_rate,
            "latency_p50_ms": p50,
            "latency_p95_ms": p95
        },
        "thresholds": {
            "min_accuracy_pct": min_accuracy,
            "min_success_rate_pct": 90.0
        },
        "case_results": case_results,
        "conclusion": {
            "accepted": accuracy >= min_accuracy && success_rate >= 90.0
        }
    });
    fs::write(&path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("Wrote {}", path.display());

    assert!(
        accuracy >= min_accuracy,
        "DeepSeek suggestion accuracy {accuracy:.2}% below threshold {min_accuracy:.2}%"
    );
    assert!(
        success_rate >= 90.0,
        "DeepSeek success rate {success_rate:.2}% below threshold 90%"
    );
}
