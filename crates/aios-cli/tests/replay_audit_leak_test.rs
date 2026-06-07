//! End-to-end leak assertion for the audit-stream artifact.
//!
//! Slice-1 added a SHA-256 fingerprint over the audit stream so we can catch
//! *any* drift; slice-2 (this test) catches the specific drift we care about
//! most: raw PII fragments from the input trace appearing in the durable
//! audit file. Together the two assertions form an equivalence-and-absence
//! pair — the hash pins what is there, this test pins what must not be.
//!
//! The trace driven here is `data/traces/sample_replay.jsonl`. Forbidden
//! substrings are the raw fields of that trace whose presence in the audit
//! stream would represent a real leak. Package names that the sanitizer
//! deliberately preserves are excluded from the forbidden list.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use aios_cli::replay::{self, Stage};

fn sample_trace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/traces/sample_replay.jsonl")
}

/// Raw substrings from `data/traces/sample_replay.jsonl` that must never
/// reach the audit stream. Each chosen so it cannot accidentally collide
/// with a SanitizedEvent field name, enum variant, or hint value:
///
/// - "张三", "张三发来" — Chinese title + body of notification sample-002
/// - "一个文件" — Chinese fragment from the same notification body
/// - "quarterly_report.pdf" — filename embedded in that body
/// - "Your verification code is", "654321" — full body + numeric code from
///   notification sample-003 (the lowercase phrase "verification code" is
///   also forbidden — the `SemanticHint::VerificationCode` enum serializes
///   as CamelCase `"VerificationCode"` so there is no collision)
/// - "0|com.ss.android.lark|42|null|10086" + fragments — the Android
///   notification_key from the NotificationInteraction event sample-006.
///   The tag portion is user-controlled (chat thread / contact name in
///   the real Lark client); the air-gap now drops the entire key.
const FORBIDDEN: &[&str] = &[
    "张三",
    "张三发来",
    "一个文件",
    "quarterly_report.pdf",
    "Your verification code is",
    "verification code",
    "654321",
    "0|com.ss.android.lark|42|null|10086",
    "|42|null|10086",
    "|null|10086",
];

fn replay_and_capture_audit() -> Vec<u8> {
    let path = sample_trace_path();
    let file =
        File::open(&path).expect("sample trace must exist at data/traces/sample_replay.jsonl");
    let reader = BufReader::new(file);

    let mut ndjson_sink: Vec<u8> = Vec::new();
    let mut audit_sink: Vec<u8> = Vec::new();

    replay::run_with_audit(
        reader,
        &mut ndjson_sink,
        &mut audit_sink,
        10,
        Stage::Execute,
    )
    .expect("replay should succeed");

    audit_sink
}

#[test]
fn audit_stream_never_contains_raw_notification_pii() {
    let audit = replay_and_capture_audit();
    let audit_text = std::str::from_utf8(&audit).expect("audit must be valid UTF-8");

    for needle in FORBIDDEN {
        assert!(
            !audit_text.contains(needle),
            "raw substring `{needle}` leaked into the audit stream — \
             this is the artifact that downstream consumers ingest, so any \
             leak here is observable PII. Audit content:\n{audit_text}",
        );
    }
}

#[test]
fn ndjson_output_never_contains_raw_notification_pii() {
    // The user-facing NDJSON sink keeps volatile uuids that the audit stream
    // strips, but its event payloads come from the same sanitized objects —
    // so the same absence guarantee must hold.
    let path = sample_trace_path();
    let file = File::open(&path).expect("sample trace must exist");
    let reader = BufReader::new(file);

    let mut ndjson_sink: Vec<u8> = Vec::new();
    let mut audit_sink: Vec<u8> = Vec::new();

    replay::run_with_audit(
        reader,
        &mut ndjson_sink,
        &mut audit_sink,
        10,
        Stage::Execute,
    )
    .expect("replay should succeed");

    let ndjson_text = std::str::from_utf8(&ndjson_sink).expect("ndjson must be valid UTF-8");
    for needle in FORBIDDEN {
        assert!(
            !ndjson_text.contains(needle),
            "raw substring `{needle}` leaked into the user-facing NDJSON sink. \
             Content:\n{ndjson_text}",
        );
    }
}
