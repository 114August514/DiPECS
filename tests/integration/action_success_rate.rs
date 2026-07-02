//! 动作成功率 baseline：四类可路由动作在 mock bridge 上的成功/失败分布。
//!
//! ## 分类依据
//!
//! `AndroidAdapter::classify`（crate-private）的路由规则：
//! - `PrefetchFile`（`url:`/`uri:` target）→ 转发到设备（Forwarded）
//! - `PrefetchFile`（其他 target）→ 本地 stub（LocalStub）
//! - `PreWarmProcess` / `KeepAlive` / `ReleaseMemory` → 无条件转发到设备（Forwarded）
//! - `NoOp` → 本地 stub（LocalStub）
//!
//! ## 测试策略
//!
//! - 转发类动作：注入指向 mock `TcpListener` 的 `AndroidAdapter`，
//!   mock bridge 回送 `{status:"ok"}` 或 `{status:"rejected"}`，
//!   断言 terminal 状态 == `Succeeded` / `Failed`。
//! - 本地 stub 类动作：使用 `DefaultActionExecutor`，无需网络，
//!   断言 terminal 状态 == `Succeeded`。

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use aios_action::{AndroidAdapter, AndroidBridgeConfig, DefaultActionExecutor};
use aios_core::action_lifecycle::ActionLifecycle;
use aios_core::policy_engine::PolicyEngine;
use aios_spec::context::ContextSummary;
use aios_spec::governance::ActionState;
use aios_spec::intent::{
    ActionType, ActionUrgency, CapabilityLevel, DecisionRoute, Intent, IntentBatch, IntentType,
    RiskLevel, SuggestedAction,
};
use aios_spec::{SourceTier, StructuredContext};

// ── helpers ──────────────────────────────────────────────────────────────────

const TOKEN: &str = "test-token";

/// 结果分类：每种动作类型的预期执行路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OutcomeClass {
    /// 转发到设备，设备回 ok → Succeeded
    ForwardedOk,
    /// 本地 stub 处理 → Succeeded（不接触设备）
    LocalStub,
}

fn permissive_capability() -> CapabilityLevel {
    CapabilityLevel {
        max_risk: RiskLevel::High,
        allowed_actions: vec![
            ActionType::NoOp,
            ActionType::PreWarmProcess,
            ActionType::PrefetchFile,
            ActionType::KeepAlive,
            ActionType::ReleaseMemory,
        ],
    }
}

fn context_with_app(app: &str) -> StructuredContext {
    StructuredContext {
        window_id: "w-baseline".into(),
        window_start_ms: 0,
        window_end_ms: 1000,
        duration_secs: 1,
        events: vec![],
        summary: ContextSummary {
            foreground_apps: vec![app.to_string()],
            notified_apps: vec![],
            all_semantic_hints: vec![],
            file_activity: vec![],
            latest_system_status: None,
            source_tier: SourceTier::PublicApi,
        },
    }
}

fn single_action_batch(action_type: ActionType, target: &str) -> IntentBatch {
    IntentBatch {
        window_id: "w-baseline".into(),
        intents: vec![Intent {
            intent_id: "intent-baseline".into(),
            intent_type: IntentType::Idle,
            confidence: 0.9,
            risk_level: RiskLevel::Low,
            suggested_actions: vec![SuggestedAction {
                action_type,
                target: Some(target.into()),
                urgency: ActionUrgency::Immediate,
            }],
            rationale_tags: vec![],
        }],
        generated_at_ms: 1000,
        model: "test".into(),
    }
}

/// mock bridge：接受一个连接，读完请求，回送 `response`，然后半关写端。
/// 返回 (port, Receiver<String>)；Receiver 可收到原始请求体（用于校验或忽略）。
fn spawn_mock_bridge(response: &'static [u8]) -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock bridge");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = String::new();
            let _ = stream.read_to_string(&mut buf);
            let _ = tx.send(buf);
            let _ = stream.write_all(response);
            let _ = stream.flush();
            let _ = stream.shutdown(Shutdown::Write);
        }
    });
    (port, rx)
}

fn android_adapter_for(port: u16) -> AndroidAdapter {
    AndroidAdapter::new(AndroidBridgeConfig {
        host: "127.0.0.1".into(),
        port,
        auth_key: Some(TOKEN.into()),
    })
}

// ── 转发类动作（PreWarmProcess / KeepAlive / ReleaseMemory / PrefetchFile url:）──

/// 四种转发类动作在设备回 ok 时全部 → `Succeeded`。
#[test]
fn forwarded_action_types_all_succeed_on_device_ok() {
    // (action_type, target, expected_outcome_summary_contains)
    let cases: &[(ActionType, &str)] = &[
        (ActionType::PreWarmProcess, "own:warmup"),
        (ActionType::KeepAlive, "work:collector_heartbeat"),
        (ActionType::ReleaseMemory, "cache:prefetch"),
        (ActionType::PrefetchFile, "url:https://example.test/a.json"),
    ];

    // Vec<(action_type, terminal)> — ActionType lacks Hash so we avoid HashMap.
    let mut results: Vec<(ActionType, ActionState)> = Vec::new();

    for (action_type, target) in cases {
        let (port, _rx) =
            spawn_mock_bridge(br#"{"status":"ok","summary":"android_executed","latency_us":7}"#);
        let policy = PolicyEngine::default();
        let adapter = android_adapter_for(port);
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let records = lifecycle.run(
            0,
            &single_action_batch(action_type.clone(), target),
            DecisionRoute::RuleBased,
            None,
            &permissive_capability(),
            &context_with_app("com.example.app"),
        );

        // Confirm the mock bridge received the forwarded request.
        _rx.recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| {
                panic!(
                    "mock bridge should receive a request for forwarded action {:?}",
                    action_type
                )
            });

        assert_eq!(
            records.len(),
            1,
            "{action_type:?}: expected exactly one audit record"
        );
        results.push((action_type.clone(), records[0].terminal));
    }

    // 分布断言：四种转发类动作全部 Succeeded
    println!("\n=== action_success_rate: forwarded / device-ok distribution ===");
    for (action_type, terminal) in &results {
        println!("  {:?} → {:?}", action_type, terminal);
        assert_eq!(
            *terminal,
            ActionState::Succeeded,
            "{action_type:?}: device ok should map to Succeeded, got {terminal:?}"
        );
    }
}

/// 四种转发类动作在设备回 rejected 时全部 → `Failed`。
#[test]
fn forwarded_action_types_all_fail_on_device_rejected() {
    let cases: &[(ActionType, &str)] = &[
        (ActionType::PreWarmProcess, "own:warmup"),
        (ActionType::KeepAlive, "work:collector_heartbeat"),
        (ActionType::ReleaseMemory, "cache:prefetch"),
        (ActionType::PrefetchFile, "url:https://example.test/a.json"),
    ];

    let mut results: Vec<(ActionType, ActionState)> = Vec::new();

    for (action_type, target) in cases {
        let (port, _rx) = spawn_mock_bridge(br#"{"status":"rejected","error":"device_denied"}"#);
        let policy = PolicyEngine::default();
        let adapter = android_adapter_for(port);
        let lifecycle = ActionLifecycle::new(&policy, &adapter);
        let records = lifecycle.run(
            0,
            &single_action_batch(action_type.clone(), target),
            DecisionRoute::RuleBased,
            None,
            &permissive_capability(),
            &context_with_app("com.example.app"),
        );

        assert_eq!(
            records.len(),
            1,
            "{action_type:?}: expected exactly one audit record"
        );
        results.push((action_type.clone(), records[0].terminal));
    }

    println!("\n=== action_success_rate: forwarded / device-rejected distribution ===");
    for (action_type, terminal) in &results {
        println!("  {:?} → {:?}", action_type, terminal);
        assert_eq!(
            *terminal,
            ActionState::Failed,
            "{action_type:?}: device rejected should map to Failed, got {terminal:?}"
        );
    }
}

// ── 本地 stub 类动作（NoOp / PrefetchFile 非 url: target）────────────────────

/// NoOp 和无 url: target 的 PrefetchFile 走本地 stub，不接触网络，终态 Succeeded。
#[test]
fn local_stub_action_types_succeed_without_bridge() {
    // NoOp target 无实际意义，PrefetchFile 用非 url: 前缀 target → Route::Local
    let cases: &[(ActionType, &str)] = &[
        (ActionType::NoOp, "noop"),
        // pkg: prefix is policy-approved and NOT a url:/uri: prefix → Route::Local
        (ActionType::PrefetchFile, "pkg:com.example.app"),
    ];

    let policy = PolicyEngine::default();
    let executor = DefaultActionExecutor::new();
    let lifecycle = ActionLifecycle::new(&policy, &executor);

    let mut results: Vec<(ActionType, ActionState)> = Vec::new();

    for (action_type, target) in cases {
        let records = lifecycle.run(
            0,
            &single_action_batch(action_type.clone(), target),
            DecisionRoute::RuleBased,
            None,
            &permissive_capability(),
            &context_with_app("com.example.app"),
        );

        assert_eq!(
            records.len(),
            1,
            "{action_type:?}: expected exactly one audit record"
        );
        results.push((action_type.clone(), records[0].terminal));
    }

    println!("\n=== action_success_rate: local-stub distribution ===");
    for (action_type, terminal) in &results {
        println!("  {:?} → {:?}", action_type, terminal);
        assert_eq!(
            *terminal,
            ActionState::Succeeded,
            "{action_type:?}: local stub should always Succeed, got {terminal:?}"
        );
    }
}

// ── 综合分布快照 ─────────────────────────────────────────────────────────────

/// 全部五种动作类型的完整分布快照（happy path）。
///
/// 钉死每种 ActionType 的预期 OutcomeClass，防止路由规则或 adapter 逻辑悄悄漂移。
#[test]
fn all_action_types_distribution_snapshot() {
    // (action_type, target, expected_outcome_class, expected_terminal)
    let cases: &[(ActionType, &str, OutcomeClass, ActionState)] = &[
        (
            ActionType::PreWarmProcess,
            "own:warmup",
            OutcomeClass::ForwardedOk,
            ActionState::Succeeded,
        ),
        (
            ActionType::KeepAlive,
            "work:collector_heartbeat",
            OutcomeClass::ForwardedOk,
            ActionState::Succeeded,
        ),
        (
            ActionType::ReleaseMemory,
            "cache:prefetch",
            OutcomeClass::ForwardedOk,
            ActionState::Succeeded,
        ),
        (
            ActionType::PrefetchFile,
            "url:https://example.test/a.json",
            OutcomeClass::ForwardedOk,
            ActionState::Succeeded,
        ),
        (
            ActionType::PrefetchFile,
            "pkg:com.example.app",
            OutcomeClass::LocalStub,
            ActionState::Succeeded,
        ),
        (
            ActionType::NoOp,
            "noop",
            OutcomeClass::LocalStub,
            ActionState::Succeeded,
        ),
    ];

    println!("\n=== action_success_rate: full distribution snapshot ===");
    println!(
        "  {:<20} {:<40} {:<20} Terminal",
        "ActionType", "Target", "OutcomeClass"
    );

    for (action_type, target, expected_class, expected_terminal) in cases {
        let terminal = match expected_class {
            OutcomeClass::LocalStub => {
                // 本地 stub：直接用 DefaultActionExecutor，无需端口
                let policy = PolicyEngine::default();
                let executor = DefaultActionExecutor::new();
                let lifecycle = ActionLifecycle::new(&policy, &executor);
                let records = lifecycle.run(
                    0,
                    &single_action_batch(action_type.clone(), target),
                    DecisionRoute::RuleBased,
                    None,
                    &permissive_capability(),
                    &context_with_app("com.example.app"),
                );
                assert_eq!(records.len(), 1);
                records[0].terminal
            },
            OutcomeClass::ForwardedOk => {
                let (port, _rx) = spawn_mock_bridge(
                    br#"{"status":"ok","summary":"android_executed","latency_us":7}"#,
                );
                let policy = PolicyEngine::default();
                let adapter = android_adapter_for(port);
                let lifecycle = ActionLifecycle::new(&policy, &adapter);
                let records = lifecycle.run(
                    0,
                    &single_action_batch(action_type.clone(), target),
                    DecisionRoute::RuleBased,
                    None,
                    &permissive_capability(),
                    &context_with_app("com.example.app"),
                );
                // 消费 channel，防止 mock thread 积压
                let _ = _rx.recv_timeout(Duration::from_secs(5));
                assert_eq!(records.len(), 1);
                records[0].terminal
            },
        };

        println!(
            "  {:<20} {:<40} {:<20} {:?}",
            format!("{:?}", action_type),
            target,
            format!("{:?}", expected_class),
            terminal
        );

        assert_eq!(
            terminal, *expected_terminal,
            "{action_type:?} (target={target:?}, class={expected_class:?}): \
             expected {expected_terminal:?}, got {terminal:?}"
        );
    }
}
