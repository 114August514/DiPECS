//! 对比基线测试:有 DiPECS vs 无 DiPECS。
//!
//! 验证价值:
//! - 隐私:无 DiPECS 时原始通知文本直接进入 LLM prompt;有 DiPECS 时只保留长度/脚本等元信息。
//! - 治理:无 DiPECS 时模型可能建议任意动作;有 DiPECS 时 PolicyEngine 按风险/capability 拦截。
//!
//! 真实 DeepSeek API 对比为可选(#[ignore]),默认只做本地 prompt/审计内容对比。

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use aios_agent::DecisionRouter;
use aios_core::collector_ingress::RustCollectorIngress;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_spec::traits::PrivacySanitizer;
use aios_spec::{CollectorEnvelope, RawEvent, SourceTier};

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
    let file = File::open(&path).unwrap_or_else(|_| panic!("open {}", path.display()));
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(&l).expect("valid JSON"))
        .collect()
}

/// 从 CollectorEvent 中提取 rawEvent 字段(JSON object)。
fn raw_event_object(evt: &serde_json::Value) -> Option<serde_json::Value> {
    evt.get("rawEvent").cloned()
}

/// 构造"无 DiPECS"的 naive prompt:把原始 JSON 直接喂给 LLM 并让其推荐动作。
fn naive_prompt(raw_events: &[serde_json::Value]) -> String {
    let events_json = serde_json::to_string(raw_events).expect("serialize");
    format!(
        "You are a phone assistant. Given these raw device events, decide what action to take.\n\
         Return JSON with intents and actions.\n\n{events_json}"
    )
}

/// 用 DiPECS 走一遍:collector envelope → sanitizer → router → 看 model input 里含不含 raw text。
fn dipecs_pipeline(events: &[serde_json::Value]) -> (String, usize, usize) {
    let sanitizer = DefaultPrivacyAirGap;
    let router = DecisionRouter::default();
    let ingress = RustCollectorIngress;

    let mut raw_texts: Vec<String> = Vec::new();
    let mut sanitized_events = Vec::new();

    for raw in events {
        if let Some(re) = raw_event_object(raw) {
            // 收集 raw_title/raw_text 作为 ground-truth 敏感内容。
            if let Some(title) = re
                .pointer("/NotificationPosted/raw_title")
                .and_then(|v| v.as_str())
            {
                raw_texts.push(title.to_string());
            }
            if let Some(text) = re
                .pointer("/NotificationPosted/raw_text")
                .and_then(|v| v.as_str())
            {
                raw_texts.push(text.to_string());
            }

            let raw_event: RawEvent = serde_json::from_value(re).expect("raw_event deserializes");
            let envelope = CollectorEnvelope {
                schema_version: "dipecs.collector.v1".into(),
                source: "baseline-test".into(),
                source_tier: SourceTier::PublicApi,
                device_trace_id: None,
                captured_at_ms: 0,
                received_at_ms: None,
                raw_event,
            };
            if let Ok(ingested) = ingress.accept(envelope) {
                let san = sanitizer.sanitize_with_tier(ingested.raw_event, ingested.source_tier);
                sanitized_events.push(san);
            }
        }
    }

    // 构建最小 StructuredContext 并走 DecisionRouter。
    let ctx = aios_spec::StructuredContext {
        window_id: "baseline-window".into(),
        window_start_ms: 0,
        window_end_ms: 60_000,
        duration_secs: 60,
        events: sanitized_events,
        summary: aios_spec::ContextSummary {
            foreground_apps: vec!["com.example.app".into()],
            notified_apps: vec!["com.example.chat".into()],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    };

    let result = router.evaluate(&ctx);
    let dipecs_input = serde_json::to_string(&result.intent_batch).expect("serialize");

    let leaks_in_dipecs = raw_texts
        .iter()
        .filter(|t| !t.is_empty() && dipecs_input.contains(t.as_str()))
        .count();

    (dipecs_input, raw_texts.len(), leaks_in_dipecs)
}

#[test]
fn baseline_privacy_and_governance_comparison() {
    let events = load_scenario_events("circuit-breaker");
    let raw_events: Vec<serde_json::Value> = events.iter().filter_map(raw_event_object).collect();

    let naive = naive_prompt(&raw_events);
    let (dipecs_input, raw_text_count, leaks_in_dipecs) = dipecs_pipeline(&events);

    // Count how many raw texts appear in naive prompt.
    let naive_leaks: HashSet<_> = raw_events
        .iter()
        .flat_map(|re| {
            [
                "/NotificationPosted/raw_title",
                "/NotificationPosted/raw_text",
            ]
            .iter()
            .filter_map(|ptr| re.pointer(ptr).and_then(|v| v.as_str()).map(String::from))
        })
        .filter(|needle| !needle.is_empty() && naive.contains(needle.as_str()))
        .collect();
    let naive_leaks = naive_leaks.len();

    println!("\n=== baseline comparison (circuit-breaker scenario) ===");
    println!("raw notification text snippets total    : {raw_text_count}");
    println!("leaked into naive cloud prompt          : {naive_leaks}");
    println!("leaked into DiPECS model input/audit    : {leaks_in_dipecs}");
    println!("naive prompt bytes                      : {}", naive.len());
    println!(
        "DiPECS intent_batch bytes               : {}",
        dipecs_input.len()
    );

    assert!(
        naive_leaks > 0,
        "naive prompt should contain at least some raw text to make the comparison meaningful"
    );
    assert_eq!(
        leaks_in_dipecs, 0,
        "DiPECS pipeline must not leak raw notification text into model input"
    );
}

/// 可选:把包含 raw_title/raw_text 的 naive prompt 发到真实 DeepSeek,
/// 演示无 DiPECS 基线时原始通知文本直接离设备的隐私风险。
/// 这是一个故意不安全的对照组,切勿复制到生产代码。
///
/// 运行方式:
///   DIPECS_CLOUD_LLM_API_KEY=sk-xxx cargo test -p aios-agent --test baseline_comparison_test cloud_baseline -- --ignored --nocapture
#[test]
#[ignore = "requires real DeepSeek API key; set DIPECS_CLOUD_LLM_API_KEY"]
fn cloud_baseline_action_suggestions() {
    use std::env;
    use std::time::Instant;

    // This intentionally demonstrates the privacy leak of a non-DiPECS baseline:
    // raw notification text leaves the device.
    let api_key = env::var("DIPECS_CLOUD_LLM_API_KEY")
        .or_else(|_| env::var("DEEPSEEK_API_KEY"))
        .expect("API key");

    let events = load_scenario_events("circuit-breaker");
    let raw_events: Vec<serde_json::Value> =
        events.iter().filter_map(raw_event_object).take(5).collect();
    let prompt = naive_prompt(&raw_events);

    let start = Instant::now();
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("http client");

    let body = serde_json::json!({
        "model": "deepseek-v4-flash",
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.1,
    });

    let resp = client
        .post("https://api.deepseek.com/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .expect("deepseek request");

    let latency_ms = start.elapsed().as_millis();
    let status = resp.status();
    let text = resp.text().expect("read response");

    println!("\n=== cloud baseline (no DiPECS) ===");
    println!("status      : {status}");
    println!("latency_ms  : {latency_ms}");
    println!("response    : {text:.500}");

    assert!(status.is_success(), "deepseek returned {status}");
}
