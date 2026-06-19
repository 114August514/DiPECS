use aios_spec::*;

#[test]
fn collector_envelope_wraps_raw_event_with_source_metadata() {
    let envelope = CollectorEnvelope {
        schema_version: "dipecs.collector.v1".into(),
        source: "apps.android-collector.usage".into(),
        source_tier: SourceTier::PublicApi,
        device_trace_id: Some("trace-1".into()),
        captured_at_ms: 1000,
        received_at_ms: Some(1005),
        raw_event: RawEvent::ScreenState(ScreenStateEvent {
            timestamp_ms: 1000,
            state: ScreenState::Interactive,
        }),
    };

    assert_eq!(envelope.schema_version, "dipecs.collector.v1");
    assert_eq!(envelope.source, "apps.android-collector.usage");
    assert!(matches!(envelope.raw_event, RawEvent::ScreenState(_)));
}

#[test]
fn action_proposal_wraps_suggested_action_with_coord_and_effect() {
    let proposal = ActionProposal {
        intent_id: "intent-1".into(),
        coord: ActionCoord {
            window_ordinal: 0,
            intent_ordinal: 0,
            action_ordinal: 0,
        },
        action: SuggestedAction {
            action_type: ActionType::NoOp,
            target: None,
            urgency: ActionUrgency::IdleTime,
        },
        effect: EffectClass::PureRead,
        proposed_at_ms: 2000,
    };

    assert_eq!(proposal.intent_id, "intent-1");
    assert_eq!(proposal.proposed_at_ms, 2000);
    assert!(matches!(proposal.action.action_type, ActionType::NoOp));
    assert!(matches!(proposal.effect, EffectClass::PureRead));
}

#[test]
fn action_state_terminal_detection() {
    assert!(ActionState::Succeeded.is_terminal());
    assert!(ActionState::RejectedInvalidSchema.is_terminal());
    assert!(!ActionState::Proposed.is_terminal());
}

#[test]
fn decision_backend_result_records_route_and_output_batch() {
    let result = DecisionBackendResult {
        route: DecisionRoute::RuleBased,
        intent_batch: IntentBatch {
            window_id: "w1".into(),
            intents: vec![],
            generated_at_ms: 3000,
            model: "rule-based-v0".into(),
        },
        rationale_tags: vec!["idle_window".into()],
        latency_us: 42,
        error: None,
    };

    assert!(matches!(result.route, DecisionRoute::RuleBased));
    assert_eq!(result.intent_batch.window_id, "w1");
    assert!(result.error.is_none());
}
