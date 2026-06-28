//! TraceEngine source_tier 回回归测试
//!
//! 验证 DefaultTraceEngine 在回放 GoldenTrace 时正确保留并比对 source_tier。

use aios_core::privacy_airgap::DefaultPrivacyAirGap;
use aios_core::trace_engine::DefaultTraceEngine;
use aios_spec::{
    AppTransition, AppTransitionRawEvent, GoldenTrace, RawEvent, SanitizedEvent,
    SanitizedEventType, SourceTier,
};

fn make_app_transition_raw_event(package_name: &str) -> RawEvent {
    RawEvent::AppTransition(AppTransitionRawEvent {
        timestamp_ms: 1000,
        package_name: package_name.into(),
        activity_class: Some("Main".into()),
        transition: AppTransition::Foreground,
    })
}

fn make_expected_sanitized_event(package_name: &str, tier: SourceTier) -> SanitizedEvent {
    SanitizedEvent {
        event_id: "expected-id".into(),
        timestamp_ms: 1000,
        event_type: SanitizedEventType::AppTransition {
            package_name: package_name.into(),
            activity_class: Some("Main".into()),
            transition: AppTransition::Foreground,
        },
        source_tier: tier,
        app_package: Some(package_name.into()),
        uid: None,
    }
}

#[test]
fn trace_engine_uses_source_tiers() {
    let golden = GoldenTrace {
        trace_id: "test-1".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        raw_events: vec![make_app_transition_raw_event("com.example.app")],
        expected_sanitized: vec![make_expected_sanitized_event(
            "com.example.app",
            SourceTier::Daemon,
        )],
        expected_intents: aios_spec::IntentBatch {
            window_id: "w1".into(),
            intents: vec![],
            generated_at_ms: 1000,
            model: "test".into(),
        },
        expected_actions: vec![],
        source_tiers: vec![SourceTier::Daemon],
    };

    let engine = DefaultTraceEngine::new(DefaultPrivacyAirGap);
    let result = engine.validate_sanitization(&golden);

    assert!(
        result.sanitization_match,
        "expected sanitization to match with Daemon tier"
    );
}

#[test]
fn trace_engine_fallback_to_public_api_for_missing_tiers() {
    let golden = GoldenTrace {
        trace_id: "test-2".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        raw_events: vec![make_app_transition_raw_event("com.example.app")],
        expected_sanitized: vec![make_expected_sanitized_event(
            "com.example.app",
            SourceTier::PublicApi,
        )],
        expected_intents: aios_spec::IntentBatch {
            window_id: "w1".into(),
            intents: vec![],
            generated_at_ms: 1000,
            model: "test".into(),
        },
        expected_actions: vec![],
        source_tiers: vec![],
    };

    let engine = DefaultTraceEngine::new(DefaultPrivacyAirGap);
    let result = engine.validate_sanitization(&golden);

    assert!(
        result.sanitization_match,
        "expected fallback to PublicApi when source_tiers is empty"
    );
}

#[test]
fn trace_engine_fallback_for_partial_tiers() {
    // Two raw events, but only one tier provided (PublicApi).
    // The second event should fall back to PublicApi.
    let golden = GoldenTrace {
        trace_id: "test-3".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        raw_events: vec![
            make_app_transition_raw_event("com.first.app"),
            make_app_transition_raw_event("com.second.app"),
        ],
        expected_sanitized: vec![
            make_expected_sanitized_event("com.first.app", SourceTier::PublicApi),
            // Second event falls back to PublicApi because source_tiers has only one entry
            make_expected_sanitized_event("com.second.app", SourceTier::PublicApi),
        ],
        expected_intents: aios_spec::IntentBatch {
            window_id: "w1".into(),
            intents: vec![],
            generated_at_ms: 1000,
            model: "test".into(),
        },
        expected_actions: vec![],
        source_tiers: vec![SourceTier::PublicApi],
    };

    let engine = DefaultTraceEngine::new(DefaultPrivacyAirGap);
    let result = engine.validate_sanitization(&golden);

    assert!(
        result.sanitization_match,
        "expected fallback to PublicApi for missing tier entries"
    );
}
