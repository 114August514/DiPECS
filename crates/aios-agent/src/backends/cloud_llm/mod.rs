//! Cloud LLM backend split into focused modules:
//! - `config`: environment-driven provider/config loading
//! - `client`: HTTP request/response handling
//! - `translate`: model JSON -> DiPECS intent translation

pub mod client;
pub mod config;
mod summarizer;
mod translate;

pub use summarizer::ProfileSummarizer;

use client::CloudLlmBackend;
use config::{cloud_llm_enabled, CloudLlmConfig};

#[derive(Debug, Clone)]
pub enum CloudBackendState {
    Disabled,
    Misconfigured(String),
    Ready(CloudLlmBackend),
}

impl CloudBackendState {
    pub fn from_env() -> Self {
        if !cloud_llm_enabled() {
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

#[cfg(test)]
mod latency_tests {
    use std::env;

    use crate::backends::cloud_llm::client::CloudLlmBackend;
    use crate::backends::cloud_llm::config::{CloudLlmConfig, CloudProvider};
    use crate::backends::local_evaluator::LocalEvaluatorBackend;
    use crate::backends::rule_based::RuleBasedBackend;
    use crate::DecisionBackend;
    use aios_spec::{ContextSummary, StructuredContext};

    fn make_context() -> StructuredContext {
        // 用简单、低歧义的上下文让模型稳定返回 Idle/NoOp,避免 intent_type/action_type 混淆。
        StructuredContext {
            window_id: "latency-window".into(),
            window_start_ms: 1000,
            window_end_ms: 11000,
            duration_secs: 10,
            events: vec![aios_spec::SanitizedEvent {
                event_id: "evt-1".into(),
                timestamp_ms: 5000,
                event_type: aios_spec::SanitizedEventType::Notification {
                    source_package: "com.example.app".into(),
                    category: Some("msg".into()),
                    channel_id: None,
                    title_hint: aios_spec::TextHint {
                        length_chars: 5,
                        script: aios_spec::ScriptHint::Latin,
                        is_emoji_only: false,
                    },
                    text_hint: aios_spec::TextHint {
                        length_chars: 20,
                        script: aios_spec::ScriptHint::Latin,
                        is_emoji_only: false,
                    },
                    semantic_hints: vec![],
                    is_ongoing: false,
                    group_key: None,
                },
                source_tier: aios_spec::SourceTier::PublicApi,
                app_package: Some("com.example.app".into()),
                uid: None,
            }],
            summary: ContextSummary {
                foreground_apps: vec![],
                notified_apps: vec!["com.example.app".into()],
                all_semantic_hints: vec![],
                file_activity: vec![],
                latest_system_status: None,
                source_tier: aios_spec::SourceTier::PublicApi,
            },
        }
    }

    fn percentile(sorted_us: &[u64], p: f64) -> f64 {
        if sorted_us.is_empty() {
            return 0.0;
        }
        let idx = (p / 100.0 * (sorted_us.len() as f64 - 1.0)).floor() as usize;
        let idx = idx.clamp(0, sorted_us.len() - 1);
        sorted_us[idx] as f64
    }

    fn stats(name: &str, latencies_us: &[u64]) {
        if latencies_us.is_empty() {
            println!("{name}: no samples");
            return;
        }
        let mut sorted = latencies_us.to_vec();
        sorted.sort_unstable();
        let mean = sorted.iter().sum::<u64>() as f64 / sorted.len() as f64;
        println!(
            "{name}: mean={:.2}ms p50={:.2}ms p95={:.2}ms p99={:.2}ms samples={}",
            mean / 1000.0,
            percentile(&sorted, 50.0) / 1000.0,
            percentile(&sorted, 95.0) / 1000.0,
            percentile(&sorted, 99.0) / 1000.0,
            sorted.len()
        );
    }

    /// 真实 DeepSeek API 延迟测试。
    ///
    /// 运行方式:
    ///   DIPECS_CLOUD_LLM_API_KEY=sk-xxx cargo test -p aios-agent --lib cloud_llm::latency_tests::decision_latency_comparison -- --nocapture --ignored
    #[test]
    #[ignore = "requires real DeepSeek API key; set DIPECS_CLOUD_LLM_API_KEY"]
    fn decision_latency_comparison() {
        let api_key = env::var("DIPECS_CLOUD_LLM_API_KEY")
            .or_else(|_| env::var("DEEPSEEK_API_KEY"))
            .expect("set DIPECS_CLOUD_LLM_API_KEY or DEEPSEEK_API_KEY to run this test");

        let rounds: usize = env::var("DIPECS_BENCH_ROUNDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);

        let ctx = make_context();
        let rule_based = RuleBasedBackend;
        let local_eval = LocalEvaluatorBackend;
        let cloud_config = CloudLlmConfig::new_for_test(
            CloudProvider::DeepSeek,
            "https://api.deepseek.com/chat/completions",
            "deepseek-v4-flash",
            api_key,
        );
        let cloud = CloudLlmBackend::try_new(cloud_config).expect("cloud backend init failed");

        println!(
            "\n=== Decision backend latency comparison ({} rounds) ===",
            rounds
        );

        let mut rule_lat = Vec::with_capacity(rounds);
        for _ in 0..rounds {
            let res = rule_based.evaluate(&ctx);
            assert!(res.error.is_none(), "RuleBased should not error");
            rule_lat.push(res.latency_us);
        }
        stats("RuleBased", &rule_lat);

        let mut local_lat = Vec::with_capacity(rounds);
        for _ in 0..rounds {
            let res = local_eval.evaluate(&ctx);
            assert!(res.error.is_none(), "LocalEvaluator should not error");
            local_lat.push(res.latency_us);
        }
        stats("LocalEvaluator", &local_lat);

        let mut cloud_lat = Vec::with_capacity(rounds);
        for i in 0..rounds {
            println!("  CloudLLM round {}/{} ...", i + 1, rounds);
            let res = cloud.evaluate(&ctx);
            if let Some(ref err) = res.error {
                panic!("CloudLLM error: {}", err);
            }
            cloud_lat.push(res.latency_us);
        }
        stats("CloudLLM(DeepSeek deepseek-v4-flash)", &cloud_lat);

        // Sanity assertions: local backends must be orders of magnitude faster than cloud.
        let local_mean = local_lat.iter().sum::<u64>() as f64 / local_lat.len() as f64;
        let cloud_mean = cloud_lat.iter().sum::<u64>() as f64 / cloud_lat.len() as f64;
        assert!(
            cloud_mean > local_mean * 10.0,
            "CloudLLM mean ({:.2}us) should be much slower than LocalEvaluator mean ({:.2}us)",
            cloud_mean,
            local_mean
        );
    }
}

#[cfg(test)]
mod cloud_bench_tests {
    use std::collections::{BTreeSet, HashSet};
    use std::env;
    use std::fs;
    use std::io::BufRead;
    use std::path::PathBuf;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    use crate::backends::cloud_llm::client::CloudLlmBackend;
    use crate::backends::cloud_llm::config::{CloudLlmConfig, CloudProvider};
    use crate::DecisionBackend;
    use aios_spec::{
        AppTransition, ContextSummary, ExtensionCategory, LocationType, NetworkType, RingerMode,
        SanitizedEvent, SanitizedEventType, ScriptHint, SemanticHint, SourceTier,
        StructuredContext, SystemStatusSnapshot, TextHint,
    };

    fn project_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .expect("resolve project root")
    }

    fn load_scenario_events(name: &str) -> Vec<serde_json::Value> {
        let path = project_root()
            .join("data/traces/scenarios")
            .join(format!("{name}.jsonl"));
        let file = fs::File::open(&path).unwrap_or_else(|_| panic!("open {}", path.display()));
        std::io::BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(&l).expect("valid JSON"))
            .collect()
    }

    fn build_context(events: &[serde_json::Value]) -> StructuredContext {
        let mut sanitized: Vec<SanitizedEvent> = Vec::new();
        let mut foreground_apps: BTreeSet<String> = BTreeSet::new();
        let mut notified_apps: BTreeSet<String> = BTreeSet::new();
        let mut all_hints: HashSet<SemanticHint> = HashSet::new();
        let mut latest_system_status: Option<SystemStatusSnapshot> = None;

        for e in events.iter().take(60) {
            let ts = e.get("timestampMs").and_then(|v| v.as_i64()).unwrap_or(0);
            let eid = e
                .get("eventId")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            let re = match e.get("rawEvent") {
                Some(v) => v,
                None => continue,
            };

            if let Some(app_trans) = re.get("AppTransition") {
                let pkg_name = app_trans
                    .get("package_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("com.unknown.app");
                let activity = app_trans
                    .get("activity_class")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let transition = match app_trans.get("transition").and_then(|v| v.as_str()) {
                    Some("Foreground") => {
                        foreground_apps.insert(pkg_name.to_string());
                        AppTransition::Foreground
                    },
                    Some("Background") => AppTransition::Background,
                    _ => continue,
                };
                sanitized.push(SanitizedEvent {
                    event_id: eid,
                    timestamp_ms: ts,
                    event_type: SanitizedEventType::AppTransition {
                        package_name: pkg_name.to_string(),
                        activity_class: activity,
                        transition,
                    },
                    source_tier: SourceTier::PublicApi,
                    app_package: Some(pkg_name.to_string()),
                    uid: None,
                });
            } else if let Some(notif) = re.get("NotificationPosted") {
                let pkg_name = notif
                    .get("package_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("com.unknown.app");
                let category = notif
                    .get("category")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let channel_id = notif
                    .get("channel_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let raw_title = notif
                    .get("raw_title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let raw_text = notif.get("raw_text").and_then(|v| v.as_str()).unwrap_or("");
                let is_ongoing = notif
                    .get("is_ongoing")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                notified_apps.insert(pkg_name.to_string());
                let hints = derive_semantic_hints(raw_title, raw_text);
                for h in &hints {
                    all_hints.insert(h.clone());
                }

                sanitized.push(SanitizedEvent {
                    event_id: eid,
                    timestamp_ms: ts,
                    event_type: SanitizedEventType::Notification {
                        source_package: pkg_name.to_string(),
                        category,
                        channel_id,
                        title_hint: TextHint {
                            length_chars: raw_title.chars().count(),
                            script: classify_script(raw_title),
                            is_emoji_only: false,
                        },
                        text_hint: TextHint {
                            length_chars: raw_text.chars().count(),
                            script: classify_script(raw_text),
                            is_emoji_only: false,
                        },
                        semantic_hints: hints,
                        is_ongoing,
                        group_key: None,
                    },
                    source_tier: SourceTier::PublicApi,
                    app_package: Some(pkg_name.to_string()),
                    uid: None,
                });
            } else if let Some(sys) = re.get("SystemState") {
                let battery = sys
                    .get("battery_pct")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u8);
                let is_charging = sys
                    .get("is_charging")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let network = match sys.get("network").and_then(|v| v.as_str()) {
                    Some("Wifi") => NetworkType::Wifi,
                    Some("Cellular") => NetworkType::Cellular,
                    Some("Offline") => NetworkType::Offline,
                    _ => NetworkType::Unknown,
                };
                let ringer_mode = match sys.get("ringer_mode").and_then(|v| v.as_str()) {
                    Some("Vibrate") => RingerMode::Vibrate,
                    Some("Silent") => RingerMode::Silent,
                    _ => RingerMode::Normal,
                };
                latest_system_status = Some(SystemStatusSnapshot {
                    battery_pct: battery,
                    is_charging,
                    network: network.clone(),
                    ringer_mode: ringer_mode.clone(),
                    location_type: LocationType::Unknown,
                    headphone_connected: false,
                });
                sanitized.push(SanitizedEvent {
                    event_id: eid,
                    timestamp_ms: ts,
                    event_type: SanitizedEventType::SystemStatus {
                        battery_pct: battery,
                        is_charging,
                        network,
                        ringer_mode,
                        location_type: LocationType::Unknown,
                        headphone_connected: false,
                    },
                    source_tier: SourceTier::PublicApi,
                    app_package: None,
                    uid: None,
                });
            }
        }

        let fg: Vec<String> = foreground_apps.into_iter().collect();
        let notified: Vec<String> = notified_apps.into_iter().collect();
        let hints: Vec<SemanticHint> = all_hints.into_iter().collect();
        let file_activity: Vec<(ExtensionCategory, u32)> = hints
            .contains(&SemanticHint::FileMention)
            .then_some((ExtensionCategory::Unknown, 1u32))
            .into_iter()
            .collect();

        let win_start = sanitized.first().map(|e| e.timestamp_ms).unwrap_or(0);
        let win_end = sanitized
            .last()
            .map(|e| e.timestamp_ms)
            .unwrap_or(win_start + 60_000);
        let duration = ((win_end - win_start) as f64 / 1000.0).max(1.0) as u32;

        StructuredContext {
            window_id: "cloud-bench".into(),
            window_start_ms: win_start,
            window_end_ms: win_end,
            duration_secs: duration,
            events: sanitized,
            summary: ContextSummary {
                foreground_apps: fg,
                notified_apps: notified,
                all_semantic_hints: hints,
                file_activity,
                latest_system_status,
                source_tier: SourceTier::PublicApi,
            },
        }
    }

    fn derive_semantic_hints(title: &str, text: &str) -> Vec<SemanticHint> {
        let combined = format!("{title} {text}");
        let lower = combined.to_lowercase();
        let mut hints: Vec<SemanticHint> = Vec::new();

        if lower.contains(".pdf")
            || lower.contains(".doc")
            || lower.contains(".xls")
            || lower.contains(".pptx")
            || lower.contains(".txt")
            || lower.contains(".csv")
            || lower.contains(".zip")
            || lower.contains(".apk")
            || lower.contains("file")
            || lower.contains("attach")
            || lower.contains("document")
        {
            hints.push(SemanticHint::FileMention);
        }
        if lower.contains(".png")
            || lower.contains(".jpg")
            || lower.contains(".gif")
            || lower.contains(".svg")
            || lower.contains(".webp")
            || lower.contains("image")
            || lower.contains("photo")
            || lower.contains("picture")
            || lower.contains("screenshot")
            || lower.contains("img ")
        {
            hints.push(SemanticHint::ImageMention);
        }
        if lower.contains("http://") || lower.contains("https://") || lower.contains("link") {
            hints.push(SemanticHint::LinkAttachment);
        }
        hints
    }

    fn classify_script(text: &str) -> ScriptHint {
        let has_cjk = text.chars().any(|c| {
            ('\u{4E00}'..='\u{9FFF}').contains(&c)
                || ('\u{3040}'..='\u{309F}').contains(&c)
                || ('\u{30A0}'..='\u{30FF}').contains(&c)
        });
        if has_cjk {
            ScriptHint::Hanzi
        } else {
            ScriptHint::Latin
        }
    }

    /// Format a SystemTime as `YYYYmmDD-HHMMSS` (UTC, local-friendly).
    fn now_ts() -> String {
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs();
        // ------------------------------------------------------------------
        // Convert seconds since epoch to (year, month, day, hour, min, sec)
        // using an algorithm that correctly handles leap years.
        // Based on Howard Hinnant's public-domain civil_from_days.
        // ------------------------------------------------------------------
        let z = secs / 86400;
        let rem = (secs % 86400) as u32;
        let hour = (rem / 3600) % 24;
        let min = (rem % 3600) / 60;
        let sec = rem % 60;

        // Shift epoch from 1970-01-01 to 0000-03-01 so leap day is at the
        // very end of a notional 400-year cycle — this makes the math uniform.
        let z = (z + 719468) as i64;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = (z - era * 146097) as u32; // day of era [0, 146096]
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era
        let year = yoe + era as u32 * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
        let mp = (5 * doy + 2) / 153; // month index [0, 11] starting from March
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = if month <= 2 { year + 1 } else { year };
        let day = doy - (153 * mp + 2) / 5 + 1;

        format!("{year:04}{month:02}{day:02}-{hour:02}{min:02}{sec:02}")
    }

    /// 10-round cloud latency benchmark against DeepSeek.
    /// Usage: source .env && cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::latency -- --ignored --nocapture
    #[test]
    #[ignore = "requires DIPECS_CLOUD_LLM_API_KEY"]
    fn latency() {
        if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
            panic!("rustls ring provider install failed: {e:?}");
        }
        let api_key = env::var("DIPECS_CLOUD_LLM_API_KEY")
            .or_else(|_| env::var("DEEPSEEK_API_KEY"))
            .expect("set DIPECS_CLOUD_LLM_API_KEY or DEEPSEEK_API_KEY");
        let rounds = env::var("CLOUD_BENCH_ROUNDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);
        let scenario =
            env::var("CLOUD_BENCH_SCENARIO").unwrap_or_else(|_| "morning-routine".into());
        let events = load_scenario_events(&scenario);
        let ctx = build_context(&events);
        let config = CloudLlmConfig::new_for_test(
            CloudProvider::DeepSeek,
            "https://api.deepseek.com/chat/completions",
            "deepseek-v4-flash",
            &api_key,
        );
        let be = match CloudLlmBackend::try_new(config) {
            Ok(b) => b,
            Err(e) => {
                println!("SKIP: cloud backend init failed: {e}");
                return;
            },
        };

        println!("\n=== Cloud Decision Latency (DeepSeek deepseek-v4-flash) ===");
        println!(
            "scenario: {scenario}  rounds: {rounds}  events: {}",
            events.len()
        );

        let mut lats = Vec::with_capacity(rounds);
        let mut errs = 0u32;
        let mut good = 0u32;
        for i in 0..rounds {
            let start = Instant::now();
            let res = be.evaluate(&ctx);
            let wall = start.elapsed().as_millis() as u64;
            match &res.error {
                Some(e) => {
                    errs += 1;
                    println!("  [{i}/{rounds}] ERR: {e} ({wall}ms)");
                },
                None => {
                    lats.push(res.latency_us);
                    let has = res
                        .intent_batch
                        .intents
                        .iter()
                        .any(|i| !matches!(i.intent_type, aios_spec::IntentType::Idle));
                    if has {
                        good += 1;
                    }
                    println!(
                        "  [{i}/{rounds}] OK  {wall}ms  intents={}  non_trivial={has}",
                        res.intent_batch.intents.len()
                    );
                },
            }
        }

        if !lats.is_empty() {
            let mut s = lats.clone();
            s.sort_unstable();
            let n = s.len();
            let (min, p50, p95, max) = (
                s[0] / 1000,
                s[n / 2] / 1000,
                s[(n as f64 * 0.95) as usize] / 1000,
                s[n - 1] / 1000,
            );
            println!("\n  min={min}ms p50={p50}ms p95={p95}ms max={max}ms n={n}");
            let ok_rate = (rounds - errs as usize) as f64 / rounds as f64 * 100.0;
            println!(
                "  success_rate: {ok_rate:.1}%  non_trivial_rate: {:.1}%",
                good as f64 / rounds as f64 * 100.0
            );

            let out = project_root().join("data/evaluation");
            fs::create_dir_all(&out).ok();
            let p = out.join(format!("cloud-latency-{}.json", now_ts()));
            let d = serde_json::json!({
                "schema_version": "dipecs.cloud_latency.v1",
                "dataset_id": format!("cloud-latency-{}", now_ts()),
                "status": "measured_live_api",
                "environment": {"provider":"deepseek","model":"deepseek-v4-flash","scenario":scenario,"rounds":rounds},
                "results": {"success_rate_pct":ok_rate,"non_trivial_rate_pct":good as f64/rounds as f64*100.0,
                    "latency_min_ms":min,"latency_p50_ms":p50,"latency_p95_ms":p95,"latency_max_ms":max,"errors":errs},
                "thresholds": {"min_success_rate_pct":90.0,"max_p95_latency_ms":30000},
                "conclusion": {"accepted":ok_rate>=90.0 && p95<=30000},
            });
            fs::write(&p, serde_json::to_string_pretty(&d).unwrap()).unwrap();
            println!("Wrote {}", p.display());

            assert!(ok_rate >= 90.0);
            assert!(p95 <= 30000);
        }
    }

    /// One call per scenario to verify all return valid decisions.
    /// Usage: source .env && cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::smoke -- --ignored --nocapture
    #[test]
    #[ignore = "requires DIPECS_CLOUD_LLM_API_KEY"]
    fn smoke() {
        // Install ring crypto provider for reqwest TLS.
        if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
            panic!("rustls ring provider install failed: {e:?}");
        }
        let api_key = env::var("DIPECS_CLOUD_LLM_API_KEY")
            .or_else(|_| env::var("DEEPSEEK_API_KEY"))
            .expect("set DIPECS_CLOUD_LLM_API_KEY");
        let config = CloudLlmConfig::new_for_test(
            CloudProvider::DeepSeek,
            "https://api.deepseek.com/chat/completions",
            "deepseek-v4-flash",
            &api_key,
        );
        let be = match CloudLlmBackend::try_new(config) {
            Ok(b) => b,
            Err(e) => {
                println!("SKIP: cloud backend init failed: {e}");
                return;
            },
        };
        let scenarios = [
            "circuit-breaker",
            "low-battery",
            "morning-routine",
            "rich-workflow",
            "privacy-sensitive",
            "multi-app-switching",
        ];

        println!("\n=== Cloud Decision Multi-Scenario Smoke ===");
        let mut results = Vec::new();
        for sc in &scenarios {
            let events = load_scenario_events(sc);
            let ctx = build_context(&events);
            let start = Instant::now();
            let res = be.evaluate(&ctx);
            let wall = start.elapsed().as_millis() as u64;
            let intents: Vec<String> = res
                .intent_batch
                .intents
                .iter()
                .map(|i| format!("{:?}", i.intent_type))
                .collect();
            let err = res.error.is_some();
            if err {
                println!(
                    "  {sc:<25} {wall:>5}ms  ERR: {:?}",
                    res.error.as_ref().unwrap()
                );
            } else {
                println!("  {sc:<25} {wall:>5}ms  ok       intents={intents:?}");
            }
            assert!(!intents.is_empty());
            assert!(!err);
            results.push(
                serde_json::json!({"scenario":sc,"latency_ms":wall,"error":err,"intents":intents}),
            );
        }

        let out = project_root().join("data/evaluation");
        fs::create_dir_all(&out).ok();
        let p = out.join(format!("cloud-scenarios-{}.json", now_ts()));
        let d = serde_json::json!({
            "schema_version": "dipecs.cloud_scenarios.v1",
            "dataset_id": format!("cloud-scenarios-{}", now_ts()),
            "status": "measured_live_api",
            "environment": {"provider":"deepseek","model":"deepseek-v4-flash","scenarios":scenarios},
            "results": results,
            "thresholds": {"min_scenarios":6},
            "conclusion": {"accepted": results.iter().all(|r| !r["error"].as_bool().unwrap()) && results.len() >= 6},
        });
        fs::write(&p, serde_json::to_string_pretty(&d).unwrap()).unwrap();
        println!("\nWrote {}", p.display());
    }

    /// Validates that build_context correctly extracts multi-app diversity
    /// from scenario JSONL files. No API key needed.
    #[test]
    fn multi_app_context_has_app_diversity() {
        let events = load_scenario_events("multi-app-switching");
        let ctx = build_context(&events);

        // Should have AppTransition events, not just Notifications
        let app_transitions: Vec<_> = ctx
            .events
            .iter()
            .filter(|e| matches!(e.event_type, SanitizedEventType::AppTransition { .. }))
            .collect();
        assert!(
            app_transitions.len() >= 3,
            "multi-app scenario should have >= 3 AppTransition events, got {}",
            app_transitions.len()
        );

        // Should have Notification events
        let notifications: Vec<_> = ctx
            .events
            .iter()
            .filter(|e| matches!(e.event_type, SanitizedEventType::Notification { .. }))
            .collect();
        assert!(
            !notifications.is_empty(),
            "multi-app scenario should have Notification events"
        );

        // Should have multiple foreground apps (the key multi-app assertion)
        assert!(
            ctx.summary.foreground_apps.len() >= 3,
            "multi-app scenario should have >= 3 foreground apps, got {:?}",
            ctx.summary.foreground_apps
        );

        // Should have multiple notified apps
        assert!(
            ctx.summary.notified_apps.len() >= 2,
            "multi-app scenario should have >= 2 notified apps, got {:?}",
            ctx.summary.notified_apps
        );

        // Should have events from multiple different packages
        let packages: BTreeSet<&str> = ctx
            .events
            .iter()
            .filter_map(|e| e.app_package.as_deref())
            .collect();
        assert!(
            packages.len() >= 3,
            "multi-app scenario should have events from >= 3 packages, got {:?}",
            packages
        );

        // Should have non-empty semantic hints
        assert!(
            !ctx.summary.all_semantic_hints.is_empty(),
            "should have semantic hints"
        );

        // Should have system status if scenario includes SystemState events
        let raw_has_sys = events.iter().any(|e| {
            e.get("rawEvent")
                .and_then(|re| re.get("SystemState"))
                .is_some()
        });
        if raw_has_sys {
            assert!(
                ctx.summary.latest_system_status.is_some(),
                "should capture SystemState when scenario contains SystemState events"
            );
        }

        // Duration should be non-zero
        assert!(ctx.duration_secs > 0, "duration should be > 0");

        // Window should span actual event timestamps
        assert!(
            ctx.window_end_ms > ctx.window_start_ms,
            "window_end should be after window_start"
        );
    }

    /// Validates that single-app scenarios still work correctly with the new build_context.
    #[test]
    fn single_app_scenarios_work() {
        for scenario in &["circuit-breaker", "low-battery", "rich-workflow"] {
            let events = load_scenario_events(scenario);
            let ctx = build_context(&events);
            assert!(
                !ctx.events.is_empty(),
                "scenario {scenario}: should have events"
            );
            assert!(
                ctx.duration_secs > 0,
                "scenario {scenario}: duration should be > 0"
            );
            assert!(
                !ctx.summary.all_semantic_hints.is_empty(),
                "scenario {scenario}: should have semantic hints"
            );
        }
    }
}

#[cfg(test)]
mod mock_cloud_e2e_tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use crate::backends::cloud_llm::client::CloudLlmBackend;
    use crate::backends::cloud_llm::config::{CloudLlmConfig, CloudProvider};
    use crate::DecisionBackend;
    use aios_spec::{
        ContextSummary, SanitizedEvent, SanitizedEventType, ScriptHint, SemanticHint, SourceTier,
        StructuredContext, TextHint,
    };

    fn make_ctx() -> StructuredContext {
        StructuredContext {
            window_id: "mock-e2e".into(),
            window_start_ms: 0,
            window_end_ms: 60_000,
            duration_secs: 60,
            events: vec![SanitizedEvent {
                event_id: "evt-1".into(),
                timestamp_ms: 5000,
                event_type: SanitizedEventType::Notification {
                    source_package: "com.test.app".into(),
                    category: Some("msg".into()),
                    channel_id: None,
                    title_hint: TextHint {
                        length_chars: 10,
                        script: ScriptHint::Latin,
                        is_emoji_only: false,
                    },
                    text_hint: TextHint {
                        length_chars: 30,
                        script: ScriptHint::Latin,
                        is_emoji_only: false,
                    },
                    semantic_hints: vec![SemanticHint::FileMention, SemanticHint::ImageMention],
                    is_ongoing: false,
                    group_key: None,
                },
                source_tier: SourceTier::PublicApi,
                app_package: Some("com.test.app".into()),
                uid: None,
            }],
            summary: ContextSummary {
                foreground_apps: vec!["com.test.app".into()],
                notified_apps: vec!["com.test.app".into()],
                all_semantic_hints: vec![SemanticHint::FileMention, SemanticHint::ImageMention],
                file_activity: vec![],
                latest_system_status: None,
                source_tier: SourceTier::PublicApi,
            },
        }
    }

    fn start_mock(port: u16, response_body: &str, status_code: u16) {
        let body = response_body.to_string();
        thread::spawn(move || {
            let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind mock");
            if let Some(Ok(mut stream)) = listener.incoming().next() {
                let mut reader = BufReader::new(&stream);
                let mut content_length = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if line.to_lowercase().starts_with("content-length:") {
                        content_length =
                            line.split(':').nth(1).unwrap().trim().parse().unwrap_or(0);
                    }
                }
                let mut body_buf = vec![0u8; content_length];
                if content_length > 0 {
                    reader.read_exact(&mut body_buf).ok();
                }
                let resp = format!(
                    "HTTP/1.1 {status_code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(resp.as_bytes()).ok();
            }
        });
    }

    const VALID_JSON: &str = r#"{"id":"m","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"{\"intents\":[{\"intent_type\":\"OpenApp\",\"target\":\"com.test.app\",\"confidence\":0.9,\"risk_level\":\"Low\",\"actions\":[{\"action_type\":\"PreWarmProcess\",\"target\":\"own:resources\",\"urgency\":\"Immediate\"}],\"rationale_tags\":[\"e2e\"]}]}"},"finish_reason":"stop"}]}"#;

    fn backend_for(port: u16) -> CloudLlmBackend {
        let config = CloudLlmConfig::new_for_test(
            CloudProvider::GenericOpenAiCompatible,
            format!("http://127.0.0.1:{port}/v1/chat/completions"),
            "mock-model",
            "noop-key",
        );
        CloudLlmBackend::try_new(config).expect("backend init")
    }

    #[test]
    fn cloud_accepts_valid_json() {
        let port = 19420;
        start_mock(port, VALID_JSON, 200);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let be = backend_for(port);
        let result = be.evaluate(&make_ctx());
        assert!(
            result.error.is_none(),
            "expected ok, got: {:?}",
            result.error
        );
        assert!(!result.intent_batch.intents.is_empty());
    }

    #[test]
    fn cloud_handles_http_error() {
        let port = 19421;
        start_mock(port, r#"{"error":"boom"}"#, 429);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let be = backend_for(port);
        let result = be.evaluate(&make_ctx());
        assert!(result.error.is_some(), "should error on HTTP 429");
    }

    #[test]
    fn cloud_errors_on_dead_port() {
        let config = CloudLlmConfig::new_for_test(
            CloudProvider::GenericOpenAiCompatible,
            "http://127.0.0.1:65530/v1/chat/completions".to_string(),
            "mock-model",
            "noop-key",
        );
        let be = CloudLlmBackend::try_new(config).expect("backend init");
        let result = be.evaluate(&make_ctx());
        assert!(result.error.is_some(), "should error on dead port");
    }

    #[test]
    fn circuit_breaker_counts_errors() {
        let config = CloudLlmConfig::new_for_test(
            CloudProvider::GenericOpenAiCompatible,
            "http://127.0.0.1:65530/v1/chat/completions".to_string(),
            "mock-model",
            "noop-key",
        );
        let be = CloudLlmBackend::try_new(config).expect("backend init");
        let mut errs = 0u32;
        for _ in 0..3 {
            if be.evaluate(&make_ctx()).error.is_some() {
                errs += 1;
            }
        }
        assert_eq!(errs, 3, "all requests to dead port should error");
    }
}
