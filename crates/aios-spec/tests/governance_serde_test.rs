use aios_spec::*;

#[test]
fn action_proposal_serde_roundtrip() {
    let proposal = ActionProposal {
        intent_id: "intent-1".into(),
        coord: ActionCoord {
            window_ordinal: 1,
            intent_ordinal: 2,
            action_ordinal: 0,
        },
        action: SuggestedAction {
            action_type: ActionType::PrefetchFile,
            target: Some("url:https://example.test/feed.json".into()),
            urgency: ActionUrgency::IdleTime,
        },
        effect: EffectClass::LocalCacheWrite,
        proposed_at_ms: 2000,
    };

    let json = serde_json::to_string(&proposal).unwrap();
    let back: ActionProposal = serde_json::from_str(&json).unwrap();

    assert_eq!(back.intent_id, "intent-1");
    assert_eq!(back.coord.window_ordinal, 1);
    assert_eq!(back.coord.intent_ordinal, 2);
    assert_eq!(back.coord.action_ordinal, 0);
    assert!(matches!(back.action.action_type, ActionType::PrefetchFile));
    assert_eq!(
        back.action.target,
        Some("url:https://example.test/feed.json".into())
    );
    assert!(matches!(back.effect, EffectClass::LocalCacheWrite));
}

#[test]
fn unknown_action_type_deserialization_rejected() {
    let json = r#"{"action_type":"UnknownAction","target":null,"urgency":"Immediate"}"#;
    let result: Result<SuggestedAction, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn audit_record_serde_roundtrip() {
    let record = AuditRecord {
        coord: ActionCoord {
            window_ordinal: 0,
            intent_ordinal: 0,
            action_ordinal: 0,
        },
        intent_id: "intent-1".into(),
        action_type: ActionType::NoOp,
        target: None,
        effect: EffectClass::PureRead,
        transitions: vec![
            ActionState::Proposed,
            ActionState::SchemaValidated,
            ActionState::PolicyChecked,
            ActionState::Dispatched,
            ActionState::Succeeded,
        ],
        terminal: ActionState::Succeeded,
        outcome: Some(ActionOutcomeSummary {
            action_type: "NoOp".into(),
            target: None,
            summary: "noop".into(),
        }),
        denial_reason: None,
        error: None,
    };

    let json = serde_json::to_string(&record).unwrap();
    let back: AuditRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back.coord, record.coord);
    assert_eq!(back.transitions.len(), 5);
    assert!(matches!(back.terminal, ActionState::Succeeded));
}

#[test]
fn policy_action_decision_serde_roundtrip() {
    let decision = PolicyActionDecision {
        intent_ordinal: 0,
        action_ordinal: 1,
        verdict: PolicyVerdict::Denied(DenialReason::ActionCapabilityDenied),
    };

    let json = serde_json::to_string(&decision).unwrap();
    let back: PolicyActionDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(back.intent_ordinal, 0);
    assert_eq!(back.action_ordinal, 1);
    assert!(matches!(
        back.verdict,
        PolicyVerdict::Denied(DenialReason::ActionCapabilityDenied)
    ));
}
