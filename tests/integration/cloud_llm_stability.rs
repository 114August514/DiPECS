//! CloudLLM 稳定性 baseline：多次调用输出一致性。
//!
//! 运行需要 DIPECS_CLOUD_LLM_API_KEY，默认 #[ignore]。

use std::collections::BTreeSet;
use std::env;

use aios_agent::{
    CloudLlmBackend, CloudLlmConfig, CloudProvider, DecisionBackend, LocalEvaluatorBackend,
    RuleBasedBackend, DEFAULT_SYSTEM_PROMPT,
};
use aios_spec::{ContextSummary, ModelInput, SourceTier, StructuredContext};

/// 稳定性统计：给定一个后端多次调用同一输入后的 JSON 失败率与意图变化率。
struct StabilityStats {
    rounds: usize,
    success: usize,
    json_failures: usize,
    intent_changes: usize,
}

impl StabilityStats {
    fn json_failure_rate(&self) -> f64 {
        if self.rounds == 0 {
            0.0
        } else {
            self.json_failures as f64 / self.rounds as f64 * 100.0
        }
    }

    /// 相邻成功调用之间意图集合发生变化的比例。确定性后端应恒为 0%。
    fn intent_variation_rate(&self) -> f64 {
        if self.success > 1 {
            self.intent_changes as f64 / (self.success - 1) as f64 * 100.0
        } else {
            0.0
        }
    }
}

/// 对任意 `DecisionBackend` 跑 `rounds` 次同一 `ModelInput`，统计稳定性指标。
fn run_stability(
    backend: &dyn DecisionBackend,
    input: &ModelInput,
    rounds: usize,
) -> StabilityStats {
    let mut success = 0usize;
    let mut json_failures = 0usize;
    let mut intent_changes = 0usize;
    let mut prev_intents: Option<BTreeSet<String>> = None;

    for _ in 0..rounds {
        let res = backend.evaluate_model_input(input);
        match &res.error {
            Some(err) => {
                // 子串启发式分类：把错误消息里含 "json" 的归为 JSON 解析/结构失败，
                // 其余归入 other_failures。这只是粗分类，不解析结构化错误码。
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
                        intent_changes += 1;
                    }
                }
                prev_intents = Some(current);
            },
        }
    }

    StabilityStats {
        rounds,
        success,
        json_failures,
        intent_changes,
    }
}

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

/// 确定性对照组：RuleBased 与 LocalEvaluator 在同一输入上恒定输出。
///
/// 这取代了此前"控制组 = 单次调用"的平凡 baseline：真正的确定性后端对同一输入
/// 反复调用，意图集合永不变化、永不产生 JSON 解析失败。与被 `#[ignore]` 的云端
/// 测试（意图变化率 > 0、偶发 JSON 失败）形成对照，量化"本地后端稳定、云端不稳定"。
#[test]
fn rule_based_and_local_evaluator_are_perfectly_stable() {
    let input = build_simple_input();
    let rounds = 10usize;

    let backends: [(&str, &dyn DecisionBackend); 2] = [
        ("RuleBased", &RuleBasedBackend),
        ("LocalEvaluator", &LocalEvaluatorBackend),
    ];

    println!("\n=== Deterministic Stability Control Group ({rounds} rounds each) ===");
    for (name, backend) in backends {
        let stats = run_stability(backend, &input, rounds);

        println!(
            "  {name:<16} success={}/{} json_failure_rate={:.2}% intent_variation_rate={:.2}%",
            stats.success,
            stats.rounds,
            stats.json_failure_rate(),
            stats.intent_variation_rate(),
        );

        assert_eq!(
            stats.success, rounds,
            "{name}: deterministic backend should never error"
        );
        assert_eq!(
            stats.intent_variation_rate(),
            0.0,
            "{name}: deterministic backend must have 0% intent variation"
        );
        assert_eq!(
            stats.json_failure_rate(),
            0.0,
            "{name}: deterministic backend must have 0% JSON failure"
        );
    }
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

    // 对照组：确定性本地后端在同一输入上应恒定输出（变化率 0、JSON 失败 0）。
    let rule_based_stats = run_stability(&RuleBasedBackend, &input, rounds);
    let local_stats = run_stability(&LocalEvaluatorBackend, &input, rounds);
    assert_eq!(
        rule_based_stats.intent_variation_rate(),
        0.0,
        "RuleBased control group must be perfectly stable"
    );
    assert_eq!(
        local_stats.intent_variation_rate(),
        0.0,
        "LocalEvaluator control group must be perfectly stable"
    );

    // 实验组：云端后端非确定性。
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

    eprintln!("\n=== CloudLLM Stability Baseline (vs deterministic control) ===");
    eprintln!("rounds:            {total}");
    eprintln!("success:           {success}");
    eprintln!("json_failures:     {json_failures}");
    eprintln!("other_failures:    {other_failures}");
    eprintln!("intent_changes:    {change_pairs}");
    eprintln!("json_failure_rate: {json_failure_rate:.2}%");
    eprintln!("intent_variation_rate: {intent_variation_rate:.2}%");
    eprintln!(
        "control (RuleBased / LocalEvaluator) intent_variation_rate: {:.2}% / {:.2}%",
        rule_based_stats.intent_variation_rate(),
        local_stats.intent_variation_rate(),
    );

    assert!(
        success > 0,
        "CloudLLM should return at least one successful result"
    );

    // 云端 JSON 失败率应受控（<= 10% 容差）。确定性后端为 0%，这是云端相对代价。
    assert!(
        json_failure_rate <= 10.0,
        "CloudLLM JSON failure rate should be <= 10%, got {json_failure_rate:.2}%"
    );

    // 云端非确定：观察到多次成功时意图变化率应 > 0（与确定性对照组的 0% 形成对照）。
    // 只在样本足够（success > 1）时断言，避免样本不足导致的假阴性。
    if success > 1 {
        assert!(
            intent_variation_rate > 0.0,
            "CloudLLM should exhibit non-zero intent variation across {success} successful calls; \
             deterministic control groups are exactly 0%"
        );
    }
}
