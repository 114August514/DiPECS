//! End-to-end golden test for capability conformance of the replay pipeline.
//!
//! Drives `data/traces/denial.jsonl` through the real pipeline (RuleBased
//! backend → policy → action accounting) and pins both the canonical-audit
//! hash and the resulting `ReplaySummary` counters.
//!
//! ## History
//!
//! This fixture was originally crafted to produce two `ActionCapabilityDenied`
//! denials. The current router sends proactive signals to LocalEvaluator, whose
//! capability allows the prefetch and prewarm actions emitted for this trace.
//!
//! As a result the replay flows with zero denials. The test pins that
//! conformance: an app-foreground + file-access + low-battery window yields
//! authorized actions and no denials. If a future backend emits an action its
//! selected capability rejects, `actions_denied` / `denial_counts` move and
//! this guard trips.
//!
//! (Positive denial-counting — that `denial_counts` is populated *when* a denial
//! occurs — is no longer reachable through the RuleBased replay and is covered
//! at the policy-engine unit level instead.)

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use aios_cli::replay::{self, ReplaySummary, Stage};
use aios_spec::DenialReason;

/// Pinned audit hash for `data/traces/denial.jsonl` replayed through
/// `Stage::Policy` with the default 10s window. See module docs.
const RECONCILED_AUDIT_HASH: &str =
    "sha256:ae7e7245f5d9d5b1421ee2f841fc40b551a7c2ef0b62f4bd3025f642abe93b37";
fn denial_trace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/traces/denial.jsonl")
}

fn replay_summary() -> ReplaySummary {
    let file =
        File::open(denial_trace_path()).expect("data/traces/denial.jsonl must exist for this test");
    let reader = BufReader::new(file);
    let mut sink: Vec<u8> = Vec::new();
    replay::run(reader, &mut sink, 10, Stage::Policy).expect("replay should succeed")
}

#[test]
fn reconciled_trace_flows_without_capability_denials() {
    let summary = replay_summary();

    // Conformance invariant (option B): the rule engine emits no
    // capability-denied actions, so the denial map is empty.
    assert_eq!(
        summary.denial_counts,
        BTreeMap::<DenialReason, u64>::new(),
        "RuleBased must not produce capability denials after reconciliation; \
         if a rule intentionally changed, update this golden alongside it."
    );

    // The LocalEvaluator path authorizes foreground prewarm/keepalive plus
    // low-battery cache release.
    assert_eq!(summary.intents_total, 2);
    assert_eq!(summary.intents_approved, 2);
    assert_eq!(summary.intents_rejected, 0);
    assert_eq!(summary.actions_authorized, 3);
    assert_eq!(summary.actions_denied, 0);
    assert_eq!(summary.events_ingested, 3);
    assert_eq!(summary.windows_closed, 1);
    assert_eq!(summary.lines_total, 3);
    assert_eq!(summary.lines_parse_error, 0);
    assert_eq!(summary.lines_skipped_no_raw_event, 0);

    assert_eq!(
        summary.audit_hash, RECONCILED_AUDIT_HASH,
        "audit_hash drifted; if intentional update RECONCILED_AUDIT_HASH"
    );
}

#[test]
fn reconciled_summary_is_stable_across_repeated_runs() {
    let a = replay_summary();
    let b = replay_summary();
    // ReplaySummary derives PartialEq; uuids live in NDJSON output only,
    // not in the summary, so identical inputs must produce identical
    // summaries.
    assert_eq!(a, b);
}
