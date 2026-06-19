//! End-to-end golden test for the policy engine's denial counters.
//!
//! Drives `data/traces/denial.jsonl` through the real pipeline
//! (RuleBased backend → policy → action denial accounting) and pins both
//! the canonical-audit hash and the resulting `ReplaySummary.denial_counts`
//! map plus the supporting intent/action counters. The two pins are
//! complementary — the hash catches any drift anywhere in the per-stage
//! records, the explicit counters give a cleaner diagnostic for the
//! policy-specific bit that this slice is about.
//!
//! The fixture is crafted so the RuleBased backend produces exactly two
//! `ActionCapabilityDenied` denials:
//!
//! 1. A `FileSystemAccess` event triggers a `HandleFile` intent whose
//!    suggested `PrefetchFile` is rejected because the RuleBased capability
//!    only allows `[NoOp, ReleaseMemory, KeepAlive]`.
//! 2. An `AppTransition.Foreground` event triggers a `SwitchToApp` intent
//!    whose `PreWarmProcess` action is rejected for the same reason; its
//!    `KeepAlive` companion is approved because `com.android.chrome` is in
//!    the observed-foreground set.
//!
//! When this golden legitimately needs to change (a new backend rule, a
//! capability allow-list edit), update the constants here together with the
//! change so the drift is caught as an intentional bump rather than a silent
//! regression.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use aios_cli::replay::{self, ReplaySummary, Stage};
use aios_spec::DenialReason;

/// Pinned audit hash for `data/traces/denial.jsonl` replayed through
/// `Stage::Policy` with the default 10s window. See module docs.
const DENIAL_AUDIT_HASH: &str =
    "sha256:2861d4cc2d5d58ccf1f9c80b518069d1637b1386dc687abdcd001b3fe9784f2b";

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
fn denial_trace_pins_action_capability_denials() {
    let summary = replay_summary();

    let mut expected: BTreeMap<DenialReason, u64> = BTreeMap::new();
    expected.insert(DenialReason::ActionCapabilityDenied, 2);
    assert_eq!(
        summary.denial_counts, expected,
        "denial_counts drifted from the pinned golden. \
         If this is intentional, update the expected map below."
    );

    // Supporting counters anchor what the denial_counts map is summarising.
    assert_eq!(summary.intents_total, 3);
    assert_eq!(summary.intents_approved, 2);
    assert_eq!(summary.intents_rejected, 1);
    assert_eq!(summary.actions_authorized, 2);
    assert_eq!(summary.actions_denied, 2);
    assert_eq!(summary.events_ingested, 3);
    assert_eq!(summary.windows_closed, 1);
    assert_eq!(summary.lines_total, 3);
    assert_eq!(summary.lines_parse_error, 0);
    assert_eq!(summary.lines_skipped_no_raw_event, 0);

    // The canonical-audit fingerprint over every per-stage record is also
    // pinned: any drift anywhere in the pipeline (sanitize, context,
    // decision, policy, execute records) trips this even if denial_counts
    // happens to stay numerically equivalent.
    assert_eq!(
        summary.audit_hash, DENIAL_AUDIT_HASH,
        "audit_hash drifted; if intentional update DENIAL_AUDIT_HASH"
    );
}

#[test]
fn denial_summary_is_stable_across_repeated_runs() {
    let a = replay_summary();
    let b = replay_summary();
    // ReplaySummary derives PartialEq; uuids live in NDJSON output only,
    // not in the summary, so identical inputs must produce identical
    // summaries.
    assert_eq!(a, b);
}
