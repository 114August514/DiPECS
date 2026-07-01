//! Cloud LLM backend split into focused modules:
//! - `config`: environment-driven provider/config loading
//! - `client`: HTTP request/response handling
//! - `translate`: model JSON -> DiPECS intent translation

mod client;
mod config;
mod summarizer;
mod translate;

pub(crate) use client::CloudLlmBackend;
pub use summarizer::ProfileSummarizer;

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
