//! 验证 DefaultActionExecutor 的动作执行逻辑

use aios_kernel::DefaultActionExecutor;
use aios_spec::traits::ActionExecutor;
use aios_spec::{ActionType, ActionUrgency, SuggestedAction};

fn make_action(action_type: ActionType, target: Option<&str>) -> SuggestedAction {
    SuggestedAction {
        action_type,
        target: target.map(|s| s.to_string()),
        urgency: ActionUrgency::Immediate,
    }
}

// ===== PreWarmProcess =====

#[test]
fn test_prewarm_with_target_succeeds() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::PreWarmProcess, Some("com.example.app"));
    let result = executor.execute(&action);
    assert!(result.success, "PreWarmProcess with target should succeed");
    assert!(result.error.is_none());
    // latency_us 在极快机器上可能为 0 (亚微妙级执行)
}


#[test]
fn test_prewarm_without_target_fails() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::PreWarmProcess, None);
    let result = executor.execute(&action);
    assert!(!result.success, "PreWarmProcess without target should fail");
    assert!(result.error.is_some());
}

// ===== PrefetchFile =====

#[test]
fn test_prefetch_file_succeeds() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::PrefetchFile, Some("/cache/hotfile.db"));
    let result = executor.execute(&action);
    assert!(result.success);
}

#[test]
fn test_prefetch_file_no_target_succeeds() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::PrefetchFile, None);
    let result = executor.execute(&action);
    assert!(
        result.success,
        "PrefetchFile without target should still succeed"
    );
}

// ===== KeepAlive =====

#[test]
fn test_keep_alive_with_target_succeeds() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::KeepAlive, Some("com.example.fg"));
    let result = executor.execute(&action);
    assert!(result.success);
}

#[test]
fn test_keep_alive_without_target_succeeds() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::KeepAlive, None);
    let result = executor.execute(&action);
    assert!(result.success, "KeepAlive without target silently skips");
}

// ===== ReleaseMemory =====

#[test]
fn test_release_memory_succeeds() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::ReleaseMemory, None);
    let result = executor.execute(&action);
    assert!(result.success);
}

#[test]
fn test_release_memory_with_target_succeeds() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::ReleaseMemory, Some("com.example.bg"));
    let result = executor.execute(&action);
    assert!(result.success);
}

// ===== NoOp =====

#[test]
fn test_noop_succeeds() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::NoOp, None);
    let result = executor.execute(&action);
    assert!(result.success);
}

// ===== execute_batch =====

#[test]
fn test_execute_batch_returns_all_results() {
    let executor = DefaultActionExecutor;
    let actions = vec![
        make_action(ActionType::PreWarmProcess, Some("com.a")),
        make_action(ActionType::PrefetchFile, Some("/tmp/cache")),
        make_action(ActionType::NoOp, None),
    ];

    let results = executor.execute_batch(&actions);
    assert_eq!(
        results.len(),
        3,
        "batch should return one result per action"
    );
    assert!(results.iter().all(|r| r.success));
}

#[test]
fn test_execute_batch_mixed_success() {
    let executor = DefaultActionExecutor;
    let actions = vec![
        make_action(ActionType::PreWarmProcess, None), // fails
        make_action(ActionType::NoOp, None),           // succeeds
    ];

    let results = executor.execute_batch(&actions);
    assert_eq!(results.len(), 2);
    assert!(
        !results[0].success,
        "PreWarmProcess without target should fail"
    );
    assert!(results[1].success, "NoOp should succeed");
}

// ===== ActionResult fields =====

#[test]
fn test_result_contains_action_name() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::NoOp, None);
    let result = executor.execute(&action);
    assert_eq!(result.action_type, "NoOp");
    assert_eq!(result.target, None);
}

#[test]
fn test_result_preserves_target() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::KeepAlive, Some("com.fg"));
    let result = executor.execute(&action);
    assert_eq!(result.target, Some("com.fg".to_string()));
}

#[test]
fn test_latency_is_microseconds() {
    let executor = DefaultActionExecutor;
    let action = make_action(ActionType::NoOp, None);
    let result = executor.execute(&action);
    // 骨架执行应该 < 1ms
    assert!(
        result.latency_us < 1000,
        "skeleton execution should be sub-ms"
    );
}
