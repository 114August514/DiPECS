//! Golden replay-hash regression for `data/traces/sample_replay.jsonl`.
//!
//! The audit hash is the SHA-256 of the canonical (sorted-key,
//! volatility-stripped) projection of every per-stage record emitted while
//! replaying the sample trace through the full pipeline. Any change to
//! sanitization, aggregation, decision routing, policy, or executor output
//! for this trace shifts the hash and surfaces here.
//!
//! When the hash legitimately needs to change (e.g. a deliberate rule update),
//! re-run the replay locally and paste the new digest into [`GOLDEN_HASH`].

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use aios_cli::replay::{self, Stage};

/// Pinned canonical-audit hash for `data/traces/sample_replay.jsonl` replayed
/// through `Stage::Execute` with the default 10s window. See module docs.
const GOLDEN_HASH: &str = "sha256:7ccb74bc536da068006569d04d90b5930d815a58036498c2a9bea33c6bef7b51";

fn sample_trace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/traces/sample_replay.jsonl")
}

#[test]
fn sample_trace_audit_hash_matches_golden() {
    let path = sample_trace_path();
    let file =
        File::open(&path).expect("sample trace must exist at data/traces/sample_replay.jsonl");
    let reader = BufReader::new(file);

    let mut ndjson_sink: Vec<u8> = Vec::new();
    let mut audit_sink: Vec<u8> = Vec::new();

    let outcome = replay::run_with_audit(
        reader,
        &mut ndjson_sink,
        &mut audit_sink,
        10,
        Stage::Execute,
    )
    .expect("replay should succeed");

    assert_eq!(
        outcome.audit_hash,
        GOLDEN_HASH,
        "canonical replay hash drifted from the pinned golden. \
         If this change is intentional, update GOLDEN_HASH in {}.",
        file!()
    );

    // Counters anchor what the hash is hashing — if these drift the hash will
    // too, but a counter mismatch is a much clearer diagnostic.
    assert_eq!(outcome.summary.lines_total, 8);
    assert_eq!(outcome.summary.events_ingested, 7);
    assert_eq!(outcome.summary.lines_skipped_no_raw_event, 1);
    assert_eq!(outcome.summary.lines_parse_error, 0);
    assert_eq!(outcome.summary.windows_closed, 2);
    assert_eq!(outcome.summary.audit_hash, GOLDEN_HASH);
}

#[test]
fn sample_trace_canonical_audit_strips_volatile_keys() {
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

    let audit_text = std::str::from_utf8(&audit_sink).expect("audit must be UTF-8");
    for forbidden in ["event_id", "window_id", "intent_id", "latency_us"] {
        assert!(
            !audit_text.contains(forbidden),
            "canonical audit must not contain volatile key `{forbidden}`; got:\n{audit_text}",
        );
    }
}

#[test]
fn audit_hash_is_stable_across_repeated_runs() {
    let path = sample_trace_path();

    let mut hashes = Vec::new();
    for _ in 0..3 {
        let file = File::open(&path).expect("sample trace must exist");
        let reader = BufReader::new(file);
        let mut ndjson_sink: Vec<u8> = Vec::new();
        let mut audit_sink: Vec<u8> = Vec::new();
        let outcome = replay::run_with_audit(
            reader,
            &mut ndjson_sink,
            &mut audit_sink,
            10,
            Stage::Execute,
        )
        .expect("replay should succeed");
        hashes.push(outcome.audit_hash);
    }

    assert!(
        hashes.windows(2).all(|w| w[0] == w[1]),
        "audit_hash must be identical across runs, got {hashes:?}"
    );
}
