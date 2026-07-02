//! CloudLLM 稳定性 baseline：多次调用输出一致性。
//!
//! 运行需要 DIPECS_CLOUD_LLM_API_KEY，默认 #[ignore]。

use std::collections::BTreeSet;
use std::env;

use aios_agent::{
    CloudLlmBackend, CloudLlmConfig, CloudProvider, DecisionBackend, DEFAULT_SYSTEM_PROMPT,
};
use aios_spec::{ContextSummary, ModelInput, SourceTier, StructuredContext};

fn build_simple_input() -> ModelInput {
    // 构造一个简单 ModelInput；具体字段参考 aios_spec::ModelInput。
    ModelInput::current_only(StructuredContext {
        window_id: "w1".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events: vec![],
        summary: ContextSummary {
            foreground_apps: vec!["com.example.chat".into()],
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    })
}

fn parse_bool_var(key: &str) -> Option<bool> {
    env::var(key)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
}

#[test]
#[ignore = "requires real DeepSeek API key"]
fn cloud_llm_outputs_are_stable_across_calls() {
    // reqwest 使用 rustls-tls，需要显式安装 ring provider。
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        panic!("rustls ring provider install failed: {e:?}");
    }

    let api_key = env::var("DIPECS_CLOUD_LLM_API_KEY")
        .or_else(|_| env::var("DEEPSEEK_API_KEY"))
        .expect("set DIPECS_CLOUD_LLM_API_KEY or DEEPSEEK_API_KEY to run this test");

    let rounds: usize = env::var("CLOUD_BENCH_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let config = CloudLlmConfig {
        provider: CloudProvider::DeepSeek,
        endpoint: env::var("DIPECS_CLOUD_LLM_ENDPOINT")
            .unwrap_or_else(|_| "https://api.deepseek.com/chat/completions".into()),
        model: env::var("DIPECS_CLOUD_LLM_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into()),
        api_key: Some(api_key),
        timeout_secs: env::var("DIPECS_CLOUD_LLM_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(15),
        temperature: env::var("DIPECS_CLOUD_LLM_TEMPERATURE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.1),
        system_prompt: env::var("DIPECS_CLOUD_LLM_SYSTEM_PROMPT")
            .unwrap_or_else(|_| DEFAULT_SYSTEM_PROMPT.to_string()),
        reasoning_effort: env::var("DIPECS_CLOUD_LLM_REASONING_EFFORT").ok(),
        enable_thinking: parse_bool_var("DIPECS_CLOUD_LLM_ENABLE_THINKING"),
    };
    let backend = CloudLlmBackend::try_new(config).expect("cloud backend init failed");

    let input = build_simple_input();
    let mut success = 0usize;
    let mut json_failures = 0usize;
    let mut change_pairs = 0usize;
    let mut prev_intents: Option<BTreeSet<String>> = None;

    for i in 0..rounds {
        let res = backend.evaluate_model_input(&input);
        match &res.error {
            Some(err) => {
                eprintln!("  [{}/{rounds}] ERR: {err}", i + 1);
                if err.to_lowercase().contains("json") {
                    json_failures += 1;
                }
            },
            None => {
                success += 1;
                let current: BTreeSet<String> = res
                    .intent_batch
                    .intents
                    .iter()
                    .map(|intent| format!("{:?}", intent.intent_type))
                    .collect();
                if let Some(ref prev) = prev_intents {
                    if prev != &current {
                        change_pairs += 1;
                    }
                }
                prev_intents = Some(current);
            },
        }
    }

    let total = rounds;
    let other_failures = total.saturating_sub(success + json_failures);
    let json_failure_rate = json_failures as f64 / total as f64 * 100.0;
    let intent_variation_rate = if success > 1 {
        change_pairs as f64 / (success - 1) as f64 * 100.0
    } else {
        0.0
    };

    eprintln!("\n=== CloudLLM Stability Baseline ===");
    eprintln!("rounds:            {total}");
    eprintln!("success:           {success}");
    eprintln!("json_failures:     {json_failures}");
    eprintln!("other_failures:    {other_failures}");
    eprintln!("intent_changes:    {change_pairs}");
    eprintln!("json_failure_rate: {json_failure_rate:.2}%");
    eprintln!("intent_variation_rate: {intent_variation_rate:.2}%");

    assert!(
        success > 0,
        "CloudLLM should return at least one successful result"
    );
}
