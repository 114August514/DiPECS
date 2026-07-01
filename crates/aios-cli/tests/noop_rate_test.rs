//! Effective-no-op instrumentation over the offline replay pipeline.
//!
//! Drives the deterministic offline pipeline (`aios_cli::replay::run`) over the
//! `data/traces/noop_matrix.jsonl` coverage corpus — one window per distinct
//! sanitized-event pattern — and classifies each window by what the pipeline
//! *actually did*, all the way through policy + the offline adapter.
//!
//! A window is an **effective no-op** when it produced no authorized,
//! non-`NoOp` action. This is stricter than "did the rule emit an intent": an
//! intent whose only action is denied by the selected route capability or by a
//! target check is still a no-op in effect.
//!
//! After the LocalEvaluator routing split, proactive actions such as
//! `PreWarmProcess` / `PrefetchFile` should route to a capability that allows
//! them. The test asserts that every window's `denied` column is empty.
//!
//! Run with output to eyeball the table:
//!   cargo test -p aios-cli --test noop_rate_test -- --nocapture
//!
//! `EXPECTED` pins today's behavior. When a future change makes a `gap:*` or
//! `blocked:*` pattern do real work, flip that row's `expect_noop` to `false` —
//! the diff then documents exactly which blind spot was closed.

use std::collections::BTreeMap;
use std::path::PathBuf;

use aios_cli::replay::{self, Stage};
use serde_json::Value;

/// Window length short enough that each 10s-spaced corpus event closes into its
/// own window, giving a 1:1 event→window→pattern mapping.
const WINDOW_SECS: u64 = 2;

/// Ordered to match `data/traces/noop_matrix.jsonl` line-for-line.
///
/// `ok:`      a rule fires AND its action survives policy → real work.
/// `blocked:` a rule fires but every action is denied (capability) → no-op.
/// `gap:`     no rule matches the signal at all → no-op.
struct Case {
    label: &'static str,
    expect_noop: bool,
}

const EXPECTED: &[Case] = &[
    Case {
        label: "ok:app_foreground_keepalive",
        expect_noop: false,
    },
    Case {
        label: "local:file_access_prefetch",
        expect_noop: false,
    },
    Case {
        label: "ok:low_battery_release_memory",
        expect_noop: false,
    },
    Case {
        label: "ok:screen_interactive_keepalive",
        expect_noop: false,
    },
    Case {
        label: "ok:file_mention_keepalive",
        expect_noop: false,
    },
    Case {
        label: "gap:app_background",
        expect_noop: true,
    },
    Case {
        label: "ok:plain_notification_keepalive",
        expect_noop: false,
    },
    Case {
        label: "ok:verification_code_keepalive",
        expect_noop: false,
    },
    Case {
        label: "ok:proc_state_release_memory",
        expect_noop: false,
    },
    // Fix 1 keeps the app warm on any notification interaction. This is
    // imperfect: the air-gap drops the tap/dismiss action, so a dismiss is
    // treated like a tap. Fix 4 (preserve interaction action) will refine it.
    Case {
        label: "ok:notification_interaction_keepalive",
        expect_noop: false,
    },
    Case {
        label: "gap:screen_off",
        expect_noop: true,
    },
    Case {
        label: "gap:battery_ok",
        expect_noop: true,
    },
    // ActivityLaunch carries no actionable target under RuleBased: the air-gap
    // nulls `source_package` (only a uid survives) and PreWarmProcess is outside
    // the RuleBased capability. The dead rule was removed (Fix 2), so a
    // binder/activity event now legitimately produces no intent. Closing this
    // gap needs collector-side uid→package resolution (tracked separately).
    Case {
        label: "gap:activity_launch_no_target",
        expect_noop: true,
    },
];

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/traces/noop_matrix.jsonl")
        .canonicalize()
        .expect("noop_matrix.jsonl corpus must exist")
}

/// What a single window's `execute` records add up to.
#[derive(Default, Clone)]
struct WindowOutcome {
    /// Non-`NoOp` actions that passed policy and reached the adapter.
    ran: Vec<String>,
    /// Non-`NoOp` actions rejected by capability / policy / schema.
    denied: Vec<String>,
}

impl WindowOutcome {
    fn is_noop(&self) -> bool {
        self.ran.is_empty()
    }
}

fn action_type(record: &Value) -> &str {
    record["action_type"].as_str().unwrap_or("?")
}

fn terminal(record: &Value) -> &str {
    record["terminal"].as_str().unwrap_or("?")
}

#[test]
fn noop_matrix_reports_rule_based_blind_spots() {
    let corpus = std::fs::read_to_string(corpus_path()).expect("read corpus");

    let mut output: Vec<u8> = Vec::new();
    replay::run(corpus.as_bytes(), &mut output, WINDOW_SECS, Stage::Execute)
        .expect("replay should succeed");

    let records: Vec<Value> = std::str::from_utf8(&output)
        .expect("output is utf-8")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();

    // decision records, in window order: index i ↔ window_ordinal i.
    let routes: Vec<String> = records
        .iter()
        .filter(|r| r["stage"] == "decision")
        .map(|r| r["route"].as_str().unwrap_or("?").to_string())
        .collect();

    // execute records carry an explicit window_ordinal; fold them per window.
    let mut outcomes: BTreeMap<u64, WindowOutcome> = BTreeMap::new();
    for rec in records.iter().filter(|r| r["stage"] == "execute") {
        let ord = rec["window_ordinal"].as_u64().unwrap_or(u64::MAX);
        let entry = outcomes.entry(ord).or_default();
        let action = action_type(rec);
        if action == "NoOp" {
            continue;
        }
        match terminal(rec) {
            "Succeeded" | "Failed" => entry.ran.push(action.to_string()),
            _ => entry.denied.push(action.to_string()),
        }
    }

    assert_eq!(
        routes.len(),
        EXPECTED.len(),
        "every corpus line must close into exactly one window; got {} for {} cases",
        routes.len(),
        EXPECTED.len(),
    );

    let fmt_list = |v: &[String]| {
        if v.is_empty() {
            "—".to_string()
        } else {
            v.join(",")
        }
    };

    let mut report = String::new();
    report.push_str("\n=== effective-no-op matrix ===\n");
    report.push_str(&format!(
        "{:<48} {:<14} {:<16} {:>6}\n",
        "pattern", "ran", "denied", "no-op"
    ));

    let mut noop_windows = 0usize;
    let mut mismatches: Vec<String> = Vec::new();

    for (i, case) in EXPECTED.iter().enumerate() {
        let outcome = outcomes.get(&(i as u64)).cloned().unwrap_or_default();
        let noop = outcome.is_noop();
        if noop {
            noop_windows += 1;
        }
        report.push_str(&format!(
            "{:<48} {:<14} {:<16} {:>6}\n",
            case.label,
            fmt_list(&outcome.ran),
            fmt_list(&outcome.denied),
            if noop { "NoOp" } else { "act" },
        ));
        if noop != case.expect_noop {
            let label = case.label;
            let expected_noop = case.expect_noop;
            let ran = fmt_list(&outcome.ran);
            let denied = fmt_list(&outcome.denied);
            mismatches.push(format!(
                "  {label} — expected no-op={expected_noop}, measured no-op={noop} (ran={ran}, denied={denied})",
            ));
        }
    }

    let total = EXPECTED.len();
    let rate = (noop_windows as f64) * 100.0 / (total as f64);
    report.push_str(&format!(
        "\neffective no-op windows: {noop_windows}/{total}  \
         ({rate:.1}% of distinct signal patterns)\n",
    ));
    report.push_str("note: corpus is a designed coverage matrix, not a traffic sample.\n");
    eprintln!("{report}");

    assert!(
        mismatches.is_empty(),
        "no-op classification drifted from EXPECTED. Update EXPECTED if a rule \
         changed on purpose.\n{}",
        mismatches.join("\n"),
    );

    // Hard invariant: every `ok:` pattern must do real work.
    // `local:` rows are expected to do real work through LocalEvaluator.
    for (i, case) in EXPECTED.iter().enumerate() {
        if let Some(rest) = case.label.strip_prefix("ok:") {
            let outcome = outcomes.get(&(i as u64)).cloned().unwrap_or_default();
            let ran = &outcome.ran;
            let denied = &outcome.denied;
            assert!(
                !outcome.is_noop(),
                "{rest} must produce an authorized action, got ran={ran:?} denied={denied:?}",
            );
        }
        if let Some(rest) = case.label.strip_prefix("local:") {
            assert_eq!(
                routes[i], "LocalEvaluator",
                "{} should route LocalEvaluator",
                rest
            );
            let outcome = outcomes.get(&(i as u64)).cloned().unwrap_or_default();
            let ran = &outcome.ran;
            let denied = &outcome.denied;
            assert!(
                !outcome.is_noop(),
                "{rest} must produce an authorized action, got ran={ran:?} denied={denied:?}",
            );
        }
    }

    // Capability-conformance invariant: no selected backend should emit an
    // action its route capability rejects.
    let denials: Vec<(usize, Vec<String>)> = EXPECTED
        .iter()
        .enumerate()
        .filter_map(|(i, _)| {
            let denied = outcomes.get(&(i as u64)).map(|o| o.denied.clone())?;
            (!denied.is_empty()).then_some((i, denied))
        })
        .collect();
    let denied_labels: Vec<_> = denials
        .iter()
        .map(|(i, d)| (EXPECTED[*i].label, d))
        .collect();
    assert!(
        denials.is_empty(),
        "RuleBased emitted capability-denied actions (option B violated): {denied_labels:?}",
    );
}
