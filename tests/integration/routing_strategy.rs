//! 路由策略 baseline：固定路由 vs DecisionRouter 动态路由。
//!
//! 目标：证明生产配置下的 `DecisionRouter` 不劣于任何固定路由：
//! 1. 当隐私敏感度（AppTransition 数量）超过默认阈值时，安全地回退到 RuleBased；
//! 2. 当隐私门允许时，富语义信号（FileMention / ImageMention / LinkAttachment）
//!    会动态升级到 LocalEvaluator；
//! 3. 通过固定路由对照组验证：动态路由在保守场景与固定 RuleBased 等价，
//!    在富信号场景优于固定 RuleBased。

use aios_agent::{DecisionBackend, DecisionRouter, LocalEvaluatorBackend, RuleBasedBackend};
use aios_core::collector_ingress::RustCollectorIngress;
use aios_core::context_builder::WindowAggregator;
use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_spec::traits::PrivacySanitizer;
use aios_spec::{
    AppTransition, CollectorEnvelope, ContextSummary, DecisionRoute, RawEvent, SanitizedEvent,
    SanitizedEventType, ScriptHint, SemanticHint, SourceTier, StructuredContext, TextHint,
};

use crate::helpers;

fn load_scenario_trace(name: &str) -> Vec<serde_json::Value> {
    let path = helpers::repo_root()
        .join("data/traces/scenarios")
        .join(format!("{name}.jsonl"));
    helpers::load_jsonl_events(path.to_str().unwrap())
}

fn sanitize_trace(events: &[serde_json::Value]) -> Vec<SanitizedEvent> {
    let ingress = RustCollectorIngress;
    let sanitizer = DefaultPrivacyAirGap;
    let mut sanitized = vec![];
    for evt in events {
        let Some(raw_value) = evt.get("rawEvent").filter(|v| !v.is_null()).cloned() else {
            continue;
        };
        let raw: RawEvent = serde_json::from_value(raw_value).unwrap();
        let envelope = CollectorEnvelope {
            schema_version: "dipecs.collector.v1".into(),
            source: "baseline".into(),
            source_tier: SourceTier::PublicApi,
            device_trace_id: None,
            captured_at_ms: evt.get("timestampMs").and_then(|v| v.as_i64()).unwrap_or(0),
            received_at_ms: None,
            raw_event: raw,
        };
        if let Ok(ingested) = ingress.accept(envelope) {
            sanitized.push(sanitizer.sanitize_with_tier(ingested.raw_event, ingested.source_tier));
        }
    }
    sanitized
}

fn build_context(events: &[SanitizedEvent], window_secs: u64) -> StructuredContext {
    assert!(!events.is_empty(), "cannot build context from empty events");
    let min_ts = events.iter().map(|e| e.timestamp_ms).min().unwrap();
    let max_ts = events.iter().map(|e| e.timestamp_ms).max().unwrap();
    let end_ms = max_ts.max(min_ts + (window_secs * 1000) as i64);

    let mut aggregator = WindowAggregator::new(window_secs, min_ts);
    for event in events {
        aggregator.push(event.clone());
    }
    aggregator
        .close(end_ms)
        .expect("non-empty aggregator should produce a context")
}

fn run_pipeline(
    events: &[serde_json::Value],
    window_secs: u64,
) -> aios_spec::DecisionBackendResult {
    let sanitized = sanitize_trace(events);
    let ctx = build_context(&sanitized, window_secs);
    DecisionRouter::default().evaluate(&ctx)
}

fn text_hint() -> TextHint {
    TextHint {
        length_chars: 10,
        script: ScriptHint::Latin,
        is_emoji_only: false,
    }
}

fn notification_event(package: &str, hints: Vec<SemanticHint>) -> SanitizedEvent {
    SanitizedEvent {
        event_id: format!("{package}-n"),
        timestamp_ms: 1000,
        event_type: SanitizedEventType::Notification {
            source_package: package.into(),
            category: None,
            channel_id: None,
            title_hint: text_hint(),
            text_hint: text_hint(),
            semantic_hints: hints,
            is_ongoing: false,
            group_key: None,
        },
        source_tier: SourceTier::PublicApi,
        app_package: Some(package.into()),
        uid: None,
    }
}

fn make_rich_semantic_context() -> StructuredContext {
    // 仅包含 1 条 AppTransition，隐私分为 1，低于默认阈值 3。
    // 两条通知分别携带 FileMention / ImageMention / LinkAttachment，构成 local-actionable 信号。
    let events = vec![
        notification_event(
            "com.example.chat",
            vec![SemanticHint::FileMention, SemanticHint::ImageMention],
        ),
        notification_event("com.example.browser", vec![SemanticHint::LinkAttachment]),
        SanitizedEvent {
            event_id: "fg".into(),
            timestamp_ms: 500,
            event_type: SanitizedEventType::AppTransition {
                package_name: "com.example.app".into(),
                activity_class: None,
                transition: AppTransition::Foreground,
            },
            source_tier: SourceTier::PublicApi,
            app_package: Some("com.example.app".into()),
            uid: None,
        },
    ];

    StructuredContext {
        window_id: "rich-low-privacy".into(),
        window_start_ms: 0,
        window_end_ms: 10_000,
        duration_secs: 10,
        events,
        summary: ContextSummary {
            foreground_apps: vec!["com.example.app".into()],
            notified_apps: vec!["com.example.chat".into(), "com.example.browser".into()],
            all_semantic_hints: vec![
                SemanticHint::FileMention,
                SemanticHint::ImageMention,
                SemanticHint::LinkAttachment,
            ],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    }
}

#[test]
fn default_router_falls_back_to_rule_based_on_high_privacy_score() {
    let events = load_scenario_trace("rich-workflow");
    let dynamic_result = run_pipeline(&events, 10);

    let sanitized = sanitize_trace(&events);
    let ctx = build_context(&sanitized, 10);
    let fixed_rule = RuleBasedBackend.evaluate(&ctx);

    assert_eq!(
        dynamic_result.route,
        DecisionRoute::RuleBased,
        "production router should fall back to RuleBased when privacy score is high"
    );
    assert!(
        fixed_rule.error.is_none(),
        "fixed RuleBased route should produce a valid result, got error: {:?}",
        fixed_rule.error
    );
    assert_eq!(
        dynamic_result.route, fixed_rule.route,
        "dynamic router should make the same safe choice as fixed RuleBased"
    );
}

#[test]
fn dynamic_router_escalates_for_rich_semantic_hints_when_privacy_gate_allows() {
    let ctx = make_rich_semantic_context();
    let dynamic_result = DecisionRouter::default().evaluate(&ctx);
    let fixed_rule = RuleBasedBackend.evaluate(&ctx);
    let fixed_local = LocalEvaluatorBackend.evaluate(&ctx);

    // 富语义信号在隐私门允许时，has_local_actionable_signal 会短路到 LocalEvaluator，
    // 因此路由是确定性的 LocalEvaluator（不会走到 CloudLlm）。
    assert_eq!(
        dynamic_result.route,
        DecisionRoute::LocalEvaluator,
        "router should escalate rich semantic hints to LocalEvaluator"
    );
    assert!(
        fixed_rule.error.is_none(),
        "fixed RuleBased route should produce a valid result, got error: {:?}",
        fixed_rule.error
    );
    assert!(
        fixed_local.error.is_none(),
        "fixed LocalEvaluator route should produce a valid result, got error: {:?}",
        fixed_local.error
    );
    assert_ne!(
        dynamic_result.route, fixed_rule.route,
        "dynamic router should adapt upward compared to fixed RuleBased"
    );
    assert_eq!(
        dynamic_result.route, fixed_local.route,
        "dynamic router should match the richer fixed backend it selected"
    );
}
