//! End-to-end JSONL replay tests.
//!
//! These tests drive `aios_cli::replay::run` directly with synthetic Android-
//! shaped `CollectorEvent` lines, then walk the NDJSON output to assert that
//! each pipeline stage behaved as expected.

use aios_cli::replay::{self, ReplaySummary, Stage};
use serde_json::Value;

const APP_TRANSITION_LINE: &str = r#"{"eventId":"evt-1","timestampMs":1000,"source":"UsageCollector","eventType":"app_transition","rawEvent":{"AppTransition":{"timestamp_ms":1000,"package_name":"com.android.chrome","activity_class":"MainActivity","transition":"Foreground"}},"rawPayload":{}}"#;

const NOTIFICATION_VERIFY_LINE: &str = r#"{"eventId":"evt-2","timestampMs":2000,"source":"NotificationCollectorService","eventType":"notification_posted","rawEvent":{"NotificationPosted":{"timestamp_ms":2000,"package_name":"com.bank.app","category":"msg","channel_id":"verify","raw_title":"Your verification code","raw_text":"Your verification code is 123456","is_ongoing":false,"group_key":null,"has_picture":false}},"rawPayload":{}}"#;

const SYSTEM_LOW_BATTERY_LINE: &str = r#"{"eventId":"evt-3","timestampMs":3000,"source":"CollectorForegroundService","eventType":"system_state","rawEvent":{"SystemState":{"timestamp_ms":3000,"battery_pct":10,"is_charging":false,"network":"Wifi","ringer_mode":"Normal","location_type":"Unknown","headphone_connected":false,"bluetooth_connected":false}},"rawPayload":{}}"#;

const ACCESSIBILITY_NO_RAW_LINE: &str = r#"{"eventId":"evt-4","timestampMs":4000,"source":"AccessibilityCollectorService","eventType":"accessibility_text","rawEvent":null,"rawPayload":{}}"#;

const PARSE_ERROR_LINE: &str = "{not valid json";

fn fixture(lines: &[&str]) -> String {
    lines.join("\n") + "\n"
}

fn run_replay(input: &str, stage: Stage) -> (Vec<Value>, ReplaySummary) {
    let mut output: Vec<u8> = Vec::new();
    let summary =
        replay::run(input.as_bytes(), &mut output, 10, stage).expect("replay should succeed");
    let records = parse_ndjson(&output);
    (records, summary)
}

fn parse_ndjson(bytes: &[u8]) -> Vec<Value> {
    std::str::from_utf8(bytes)
        .expect("output is utf-8")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each NDJSON line parses"))
        .collect()
}

fn records_by_stage<'a>(records: &'a [Value], stage: &str) -> Vec<&'a Value> {
    records
        .iter()
        .filter(|r| r.get("stage").and_then(Value::as_str) == Some(stage))
        .collect()
}

#[test]
fn ingest_stage_emits_one_record_per_valid_line() {
    let input = fixture(&[
        APP_TRANSITION_LINE,
        NOTIFICATION_VERIFY_LINE,
        SYSTEM_LOW_BATTERY_LINE,
        ACCESSIBILITY_NO_RAW_LINE,
    ]);
    let (records, summary) = run_replay(&input, Stage::Ingest);

    let ingests = records_by_stage(&records, "ingest");
    assert_eq!(ingests.len(), 3, "skip the accessibility null-raw line");
    assert_eq!(summary.lines_total, 4);
    assert_eq!(summary.lines_skipped_no_raw_event, 1);
    assert_eq!(summary.events_ingested, 3);

    let kinds: Vec<&str> = ingests
        .iter()
        .map(|r| r["raw_event_kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec!["AppTransition", "NotificationPosted", "SystemState"]
    );

    for r in &ingests {
        assert_eq!(r["source_tier"].as_str(), Some("PublicApi"));
    }
}

#[test]
fn sanitize_stage_drops_pii_and_emits_verification_hint() {
    let input = fixture(&[NOTIFICATION_VERIFY_LINE]);
    let (records, _) = run_replay(&input, Stage::Sanitize);

    let sanitizes = records_by_stage(&records, "sanitize");
    assert_eq!(sanitizes.len(), 1);
    let sanitized = &sanitizes[0]["sanitized"];

    // Source tier from the envelope must survive into SanitizedEvent.
    assert_eq!(sanitized["source_tier"].as_str(), Some("PublicApi"));

    // The original raw_title text must not appear anywhere in the NDJSON.
    let serialized = serde_json::to_string(&records).unwrap();
    assert!(
        !serialized.contains("123456"),
        "raw verification code must not leak through sanitizer; got: {serialized}"
    );

    // VerificationCode semantic hint must be present.
    let hints = sanitized["event_type"]["Notification"]["semantic_hints"]
        .as_array()
        .expect("semantic_hints array");
    assert!(
        hints.iter().any(|h| h.as_str() == Some("VerificationCode")),
        "expected VerificationCode hint, got {hints:?}"
    );
}

#[test]
fn policy_stage_authorizes_low_battery_release_memory_intent() {
    let input = fixture(&[SYSTEM_LOW_BATTERY_LINE]);
    let (records, summary) = run_replay(&input, Stage::Policy);

    let policies = records_by_stage(&records, "policy");
    assert!(
        !policies.is_empty(),
        "expected at least one policy decision, got {records:#?}"
    );

    let release_memory_authorized = policies.iter().any(|d| {
        d["approved"].as_bool() == Some(true)
            && d["approved_actions"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .any(|a| a["action"]["action_type"].as_str() == Some("ReleaseMemory"))
                })
                .unwrap_or(false)
    });
    assert!(
        release_memory_authorized,
        "low-battery context should authorize a ReleaseMemory action; got {policies:#?}"
    );

    assert!(summary.intents_total >= 1);
    assert!(summary.actions_authorized >= 1);
    assert_eq!(summary.windows_closed, 1);
}

#[test]
fn parse_errors_are_recorded_not_propagated() {
    let input = fixture(&[
        APP_TRANSITION_LINE,
        PARSE_ERROR_LINE,
        SYSTEM_LOW_BATTERY_LINE,
    ]);
    let (records, summary) = run_replay(&input, Stage::Ingest);

    let errors = records_by_stage(&records, "error");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["line"].as_u64(), Some(2));
    assert_eq!(summary.lines_parse_error, 1);
    assert_eq!(summary.events_ingested, 2);
}

#[test]
fn replay_is_deterministic_across_runs() {
    let input = fixture(&[
        APP_TRANSITION_LINE,
        NOTIFICATION_VERIFY_LINE,
        SYSTEM_LOW_BATTERY_LINE,
    ]);
    let (_, a) = run_replay(&input, Stage::Policy);
    let (_, b) = run_replay(&input, Stage::Policy);

    // event_ids and window_ids are uuids so the JSON output differs, but the
    // aggregate counters must match exactly.
    assert_eq!(a, b);
}

#[test]
fn summary_record_is_always_last() {
    let input = fixture(&[APP_TRANSITION_LINE, SYSTEM_LOW_BATTERY_LINE]);
    let (records, _) = run_replay(&input, Stage::Policy);

    let last = records.last().expect("at least the summary record");
    assert_eq!(last["stage"].as_str(), Some("summary"));
}
