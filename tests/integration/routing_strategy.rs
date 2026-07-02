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

// ── HardcodedRouter：早期简单系统的固定路由对照组 ──────────────────────────────
//
// 复现 DiPECS 之前一个更简单系统的路由策略：
//   - `privacy_score > 3` => `RuleBased`
//   - 否则 => `LocalEvaluator`
//   - 无熔断器、无基于复杂度的云端路由。
//
// 关键：`privacy_score` 必须与生产 `DecisionRouter` 使用**完全相同**的输入，否则
// 对照组失去意义。生产实现见 `crates/aios-agent/src/router.rs`
// `DecisionRouter::compute_privacy_score`（crate-private，无法直接复用），此处**忠实
// 复刻**其逻辑：
//   - Notification 事件中每个 `VerificationCode` / `FinancialContext` 语义提示 +1
//   - 每个 `AppTransition` 事件 +1
//   - 其它事件 +0
struct HardcodedRouter;

impl HardcodedRouter {
    /// 与生产 `DecisionRouter::compute_privacy_score` 逐字段一致的隐私分。
    fn compute_privacy_score(context: &StructuredContext) -> usize {
        context
            .events
            .iter()
            .map(|event| match &event.event_type {
                SanitizedEventType::Notification { semantic_hints, .. } => semantic_hints
                    .iter()
                    .filter(|h| {
                        matches!(
                            h,
                            SemanticHint::VerificationCode | SemanticHint::FinancialContext
                        )
                    })
                    .count(),
                SanitizedEventType::AppTransition { .. } => 1,
                _ => 0,
            })
            .sum()
    }

    /// 固定路由：隐私分 > 3 => RuleBased，否则 LocalEvaluator。阈值与
    /// `RouterConfig::default().privacy_score_threshold`（3）保持一致。
    fn determine_route(context: &StructuredContext) -> DecisionRoute {
        if Self::compute_privacy_score(context) > 3 {
            DecisionRoute::RuleBased
        } else {
            DecisionRoute::LocalEvaluator
        }
    }
}

/// 从 rationale 标签中解析生产 router emit 的隐私分。
///
/// 生产 `RoutingReason::PrivacySensitive` 的标签形如
/// `routing:privacy_sensitive(score=7)`（见 router.rs `RoutingReason::tag`）。
/// 用于把生产实际计算的分数与本地复刻做数值比对。
fn parse_privacy_score_tag(tags: &[String]) -> Option<usize> {
    const PREFIX: &str = "routing:privacy_sensitive(score=";
    tags.iter().find_map(|tag| {
        tag.strip_prefix(PREFIX)
            .and_then(|rest| rest.strip_suffix(')'))
            .and_then(|num| num.parse::<usize>().ok())
    })
}

/// 将脱敏事件按 `window_secs` 时间窗切分为多个 `StructuredContext`。
///
/// 忠实复现 daemon `run_processing_loop` 的行为（`crates/aios-daemon/src/pipeline.rs`）：
/// 事件按时间戳排序后逐个 push，当事件时间戳越过当前窗口边界时先 `close` 当前窗口，
/// 再把该事件放入新窗口；最后 flush 尾窗口。空窗口不产生 context（与 daemon 一致）。
fn build_windows(events: &[SanitizedEvent], window_secs: u64) -> Vec<StructuredContext> {
    if events.is_empty() {
        return vec![];
    }
    let mut sorted = events.to_vec();
    sorted.sort_by_key(|e| e.timestamp_ms);

    let window_ms = (window_secs * 1000) as i64;
    let start = sorted.first().unwrap().timestamp_ms;

    let mut contexts = vec![];
    let mut aggregator = WindowAggregator::new(window_secs, start);
    let mut window_end = start + window_ms;

    for event in sorted {
        // 事件越过当前窗口边界：先关窗（可能跨越多个空窗口）。
        while event.timestamp_ms >= window_end {
            if let Some(ctx) = aggregator.close(window_end) {
                contexts.push(ctx);
            }
            window_end += window_ms;
        }
        aggregator.push(event);
    }
    // flush 最后一个非空窗口。
    if let Some(ctx) = aggregator.close(window_end) {
        contexts.push(ctx);
    }
    contexts
}

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

/// 固定路由对照组：`HardcodedRouter` 用与生产完全相同的隐私分做二元路由。
///
/// 两个场景合起来证明：生产 `DecisionRouter` **至少不劣于**简单固定策略——
/// - 高隐私（rich-workflow）：两者都选 RuleBased（安全一致）；
/// - 低隐私富语义：两者都选 LocalEvaluator。
///
/// 同时严格优于任何**单一固定后端**：没有哪个固定后端能在两个场景都做对
///（固定 RuleBased 在低隐私场景不会升级，固定 LocalEvaluator 在高隐私场景不安全）。
#[test]
fn hardcoded_routing_matches_dynamic_router_on_both_scenarios() {
    // 场景 1：高隐私 rich-workflow trace —— 隐私分远超阈值 3，固定路由选 RuleBased。
    let events = load_scenario_trace("rich-workflow");
    let sanitized = sanitize_trace(&events);
    let ctx = build_context(&sanitized, 10);

    let privacy_score = HardcodedRouter::compute_privacy_score(&ctx);
    assert!(
        privacy_score > 3,
        "rich-workflow context should have a high privacy score (>3), got {privacy_score}"
    );

    let dynamic = DecisionRouter::default().evaluate(&ctx);
    let hardcoded = HardcodedRouter::determine_route(&ctx);
    assert_eq!(
        hardcoded,
        DecisionRoute::RuleBased,
        "hardcoded router should pick RuleBased on high-privacy trace"
    );
    assert_eq!(
        dynamic.route, hardcoded,
        "production router should match hardcoded RuleBased choice on high-privacy trace"
    );

    // 数值分歧守卫：生产 router 在隐私敏感路径会 emit `routing:privacy_sensitive(score=N)`
    // 标签（见 router.rs RoutingReason::PrivacySensitive）。解析该 N 并与本地复刻的
    // compute_privacy_score 逐数值比对——即便未来生产分数漂移但仍未翻转路由，这里也会
    // 立即失败，避免复刻悄悄偏离 crate-private 的原实现。
    let production_score = parse_privacy_score_tag(&dynamic.rationale_tags).expect(
        "high-privacy route should emit a routing:privacy_sensitive(score=N) rationale tag",
    );
    assert_eq!(
        production_score, privacy_score,
        "replicated HardcodedRouter privacy score must equal production score \
         (production emitted {production_score}, replica computed {privacy_score})"
    );

    // 场景 2：低隐私富语义合成上下文 —— 隐私分 = 1（单个 AppTransition），
    // 固定路由与生产路由都应选 LocalEvaluator。
    let rich = make_rich_semantic_context();
    let rich_score = HardcodedRouter::compute_privacy_score(&rich);
    assert!(
        rich_score <= 3,
        "rich-semantic low-privacy context should stay under threshold, got {rich_score}"
    );

    let dynamic_rich = DecisionRouter::default().evaluate(&rich);
    let hardcoded_rich = HardcodedRouter::determine_route(&rich);
    assert_eq!(
        hardcoded_rich,
        DecisionRoute::LocalEvaluator,
        "hardcoded router should pick LocalEvaluator on low-privacy context"
    );
    assert_eq!(
        dynamic_rich.route, hardcoded_rich,
        "production router should match hardcoded LocalEvaluator choice on low-privacy context"
    );
}

/// 云端规避率 baseline（性能价值链路）。
///
/// 把三条场景 trace（morning-routine / multi-app-switching / rich-workflow）按 10s
/// 窗口切分，逐窗口过 `DecisionRouter::default()`（未配置云端 key），统计路由到本地
/// 后端（RuleBased / LocalEvaluator）与云端/降级（CloudLlm / FallbackNoOp）的比例。
///
/// ## 诚实声明：本配置下规避率结构性地为 100%
///
/// 未配置云端 key 时 `cloud_route_or_fallback` 永远不会返回 `CloudLlm`（云端后端为
/// `None`），且这些真实 trace 不会连续触发 5 次后端错误，熔断器不会跳闸到
/// `FallbackNoOp`。因此在**当前配置下**规避率恒为 100%，`>= 80%` 断言在此配置**不可
/// 证伪**——它只在配置了云端 key（`DIPECS_CLOUD_LLM_ENABLED=1` + api key）后，高复杂度
/// 窗口开始路由到 `CloudLlm` 时才变得可能失败。
///
/// 那么本测试的价值在于：(1) 确认默认（无云端）router 在真实逐窗口分布上始终保持本地；
/// (2) 覆盖 `build_windows` 的多窗口切分逻辑。依据：云端 p50 约 7-11s，本地后端亚毫秒，
/// 规避云端是最直接的用户可见延迟收益——但**不要**把这里的 100% 当作动态路由的功劳，
/// 它是"未配置云端"的结构性结果。
#[test]
fn cloud_avoidance_rate_is_high() {
    let scenarios = ["morning-routine", "multi-app-switching", "rich-workflow"];
    let router = DecisionRouter::default();

    let mut total_windows = 0usize;
    let mut avoided = 0usize; // RuleBased | LocalEvaluator
    let mut cloud_or_fallback = 0usize; // CloudLlm | FallbackNoOp

    println!("\n=== routing_strategy: cloud-avoidance per-scenario window routing ===");
    for name in scenarios {
        let events = load_scenario_trace(name);
        let sanitized = sanitize_trace(&events);
        let windows = build_windows(&sanitized, 10);
        assert!(
            !windows.is_empty(),
            "{name}: expected at least one non-empty window"
        );

        let mut scenario_avoided = 0usize;
        for ctx in &windows {
            let route = router.evaluate(ctx).route;
            match route {
                DecisionRoute::RuleBased | DecisionRoute::LocalEvaluator => {
                    avoided += 1;
                    scenario_avoided += 1;
                },
                DecisionRoute::CloudLlm | DecisionRoute::FallbackNoOp => {
                    cloud_or_fallback += 1;
                },
                // `Mock` 只由测试专用后端产生（见 router.rs with_backends），生产
                // DecisionRouter::default() 的真实后端永不 emit 它；若出现即为回归。
                DecisionRoute::Mock => {
                    panic!("default DecisionRouter should never route to Mock, got a Mock window");
                },
            }
            total_windows += 1;
        }
        println!(
            "  {name:<22} windows={:<4} avoided={scenario_avoided}",
            windows.len()
        );
    }

    assert!(total_windows > 0, "expected windows across scenarios");
    let avoidance_rate = avoided as f64 / total_windows as f64 * 100.0;
    println!(
        "  TOTAL windows={total_windows} avoided={avoided} cloud_or_fallback={cloud_or_fallback}"
    );
    println!("  cloud_avoidance_rate: {avoidance_rate:.2}%");

    assert!(
        avoidance_rate >= 80.0,
        "cloud avoidance rate should be >= 80%, got {avoidance_rate:.2}% \
         (avoided={avoided}, total={total_windows})"
    );
}
