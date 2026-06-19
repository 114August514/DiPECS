//! 验证 DefaultActionExecutor / OfflineAdapter 的 ActionAdapter 实现

use aios_action::{DefaultActionExecutor, OfflineAdapter};
use aios_core::governance::ActionAdapter;
use aios_spec::governance::{ActionCoord, ActionOutcomeSummary, ActionProposal, EffectClass};
use aios_spec::intent::{ActionType, ActionUrgency, SuggestedAction};

fn make_authorized(action_type: ActionType, target: Option<&str>) -> aios_core::governance::AuthorizedAction {
    let effect = EffectClass::from_action_type(&action_type);
    let proposal = ActionProposal {
        intent_id: "intent-test".into(),
        coord: ActionCoord {
            window_ordinal: 0,
            intent_ordinal: 0,
            action_ordinal: 0,
        },
        action: SuggestedAction {
            action_type,
            target: target.map(|s| s.to_string()),
            urgency: ActionUrgency::Immediate,
        },
        effect,
        proposed_at_ms: 1000,
    };
    aios_core::governance::AuthorizedAction::seal_for_test(&proposal, 2000)
}

// ===== DefaultActionExecutor =====

#[test]
fn test_prewarm_with_target_succeeds() {
    let executor = DefaultActionExecutor::new();
    let action = make_authorized(ActionType::PreWarmProcess, Some("com.example.app"));
    let outcome = executor.execute(&action).unwrap();
    assert!(outcome.success);
    assert_eq!(outcome.action_type, "PreWarmProcess");
}

#[test]
fn test_prewarm_without_target_fails() {
    let executor = DefaultActionExecutor::new();
    let action = make_authorized(ActionType::PreWarmProcess, None);
    let result = executor.execute(&action);
    assert!(result.is_err());
}

#[test]
fn test_prefetch_file_succeeds() {
    let executor = DefaultActionExecutor::new();
    let action = make_authorized(ActionType::PrefetchFile, Some("/cache/hotfile.db"));
    let outcome = executor.execute(&action).unwrap();
    assert!(outcome.success);
}

#[test]
fn test_keep_alive_with_target_succeeds() {
    let executor = DefaultActionExecutor::new();
    let action = make_authorized(ActionType::KeepAlive, Some("com.example.fg"));
    let outcome = executor.execute(&action).unwrap();
    assert!(outcome.success);
    assert_eq!(outcome.target, Some("com.example.fg".to_string()));
}

#[test]
fn test_release_memory_succeeds() {
    let executor = DefaultActionExecutor::new();
    let action = make_authorized(ActionType::ReleaseMemory, None);
    let outcome = executor.execute(&action).unwrap();
    assert!(outcome.success);
}

#[test]
fn test_noop_succeeds() {
    let executor = DefaultActionExecutor::new();
    let action = make_authorized(ActionType::NoOp, None);
    let outcome = executor.execute(&action).unwrap();
    assert!(outcome.success);
    assert_eq!(outcome.summary, "noop");
}

// ===== OfflineAdapter =====

#[test]
fn offline_adapter_covers_all_action_types() {
    let adapter = OfflineAdapter;
    let cases = vec![
        (ActionType::NoOp, None),
        (ActionType::PreWarmProcess, Some("com.example.app")),
        (ActionType::PrefetchFile, Some("url:https://example.test/feed.json")),
        (ActionType::KeepAlive, Some("com.example.app")),
        (ActionType::ReleaseMemory, None),
    ];

    for (action_type, target) in cases {
        let authorized = make_authorized(action_type, target);
        let outcome = adapter.execute(&authorized).unwrap();
        assert!(outcome.success);
        assert_eq!(outcome.latency_us, 0);
        assert!(!outcome.summary.is_empty());
    }
}

#[test]
fn offline_outcome_summary_is_deterministic() {
    let adapter = OfflineAdapter;
    let authorized = make_authorized(ActionType::PrefetchFile, Some("url:https://x.test/"));
    let a = adapter.execute(&authorized).unwrap();
    let b = adapter.execute(&authorized).unwrap();
    assert_eq!(
        ActionOutcomeSummary::from_outcome(&a),
        ActionOutcomeSummary::from_outcome(&b)
    );
}

// ===== adapter name =====

#[test]
fn default_adapter_name_is_default() {
    let executor = DefaultActionExecutor::new();
    assert_eq!(executor.name(), "default");
}

#[test]
fn offline_adapter_name_is_offline() {
    let adapter = OfflineAdapter;
    assert_eq!(adapter.name(), "offline");
}
