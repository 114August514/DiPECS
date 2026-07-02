//! The honest headline claim, encoded as an assertion: DiPECS's *personalized*
//! next-app models beat the *non-personalized popularity* baselines on the real
//! LSApp dataset — on both the standard split and the harder cold-start
//! (held-out user) split.
//!
//! This is a spec-as-assertion test. It trains and evaluates the full
//! next-app pipeline (`aios_cli::next_app::{train, evaluate}`) on the real
//! 3.66M-row LSApp trace and asserts, per split, that the personalized models
//! that genuinely win there strictly outrank *both* popularity baselines
//! (`global_popularity` and `mfu`) on hit@1 AND MRR@5.
//!
//! The claim is deliberately not "ensemble always wins": the winner differs by
//! split, and this test asserts against whichever personalized model actually
//! wins each one.
//!   * Standard split — `ensemble` and `markov` each beat both popularity
//!     baselines. (Ensemble is best overall.)
//!   * Cold-start split — the best of {`markov`, `xgboost`} beats both
//!     popularity baselines. (XGBoost is best; `mfu` degenerates to
//!     `global_popularity` for unseen users, and `naive_bayes` / `ensemble`
//!     collapse below popularity here — so we do NOT assert on those.)
//!
//! ## Full dataset, not a subset
//! We run the full LSApp trace. A bounded prefix was measured and rejected:
//! on subsets the tree models (`xgboost`, and therefore `ensemble`) are
//! undertrained and *collapse* on the cold-start split (e.g. xgboost cold-start
//! hit@1 falls from 28.9% on full data to ~4% on a 1M-row prefix), which would
//! invert the very claim under test. A subset also still requires the same
//! 157 MB `lsapp.tsv` on disk, so it buys no CI portability — only the full
//! run reproduces the committed reports bit-for-bit and is deterministic.
//!
//! ## Runtime / how to run
//! Full train+eval per split is ~24s in release, ~2min in debug. The canonical
//! invocation is release:
//!   cargo test -p aios-cli --release --test next_app_lsapp_test -- --nocapture
//! In a debug build the test skips LOUDLY (pointing at `--release`) unless
//! `DIPECS_NEXT_APP_EVAL_FORCE=1` is set. This is a documented conditional skip,
//! never a silent pass.
//!
//! ## Data availability
//! `data/lsapp/lsapp.tsv` is git-ignored (157 MB) and only exists after
//! `git submodule update --init third_party/LSApp && bash tools/prepare-lsapp.sh`.
//! When it is absent the test skips LOUDLY and returns — it does NOT pass
//! silently, so a runner without the fixture cannot manufacture a fake green.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aios_cli::next_app::{
    evaluate, train, EvalOptions, NextAppDataset, NextAppSplit, TrainOptions,
};
use serde_json::Value;

/// Match the CLI defaults used to generate the committed reports
/// (`data/evaluation/lsapp-*.report.json`), so this test reproduces them.
const HORIZON_SECS: u64 = 30;
const HISTORY_LEN: usize = 5;

/// Minimum strict margin by which a personalized model must beat *each*
/// popularity baseline. Chosen below HALF the smallest measured full-LSApp
/// delta on either split, so the claim stays a strict improvement with
/// headroom against minor nondeterminism.
///
/// Smallest measured deltas over BOTH baselines, per split (committed reports):
///   * standard:   markov−mfu = 11.29 pp hit@1, 0.090 MRR@5.
///   * cold-start: best{markov,xgboost}−popularity = 12.04 pp hit@1, 0.073 MRR@5.
///
/// Half of the tighter (standard) case is ~5.6 pp / ~0.045; we sit below both.
const HIT1_MARGIN_PP: f64 = 5.0;
const MRR5_MARGIN: f64 = 0.03;

/// The two non-personalized baselines the personalized models must beat.
const POPULARITY_BASELINES: &[&str] = &["global_popularity", "mfu"];

fn lsapp_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/lsapp/lsapp.tsv")
}

#[derive(Clone, Copy)]
struct Metrics {
    hit1: f64,
    mrr5: f64,
}

fn metrics_of(report: &Value, ranker: &str) -> Metrics {
    let node = &report["metrics"][ranker];
    Metrics {
        hit1: node["hit_rate_at_1_pct"]
            .as_f64()
            .unwrap_or_else(|| panic!("report missing hit_rate_at_1_pct for `{ranker}`")),
        mrr5: node["mean_reciprocal_rank_at_5"]
            .as_f64()
            .unwrap_or_else(|| panic!("report missing mean_reciprocal_rank_at_5 for `{ranker}`")),
    }
}

/// True iff `challenger` strictly beats every baseline by the margins on BOTH
/// hit@1 and MRR@5.
fn beats_all(report: &Value, challenger: &str, baselines: &[&str]) -> bool {
    let c = metrics_of(report, challenger);
    baselines.iter().all(|base| {
        let b = metrics_of(report, base);
        c.hit1 > b.hit1 + HIT1_MARGIN_PP && c.mrr5 > b.mrr5 + MRR5_MARGIN
    })
}

/// Hard assertion form: `challenger` must beat every baseline by the margins on
/// both metrics, with a diagnostic message naming the exact numbers on failure.
fn assert_beats_all(report: &Value, challenger: &str, baselines: &[&str], split_label: &str) {
    let c = metrics_of(report, challenger);
    for base in baselines {
        let b = metrics_of(report, base);
        assert!(
            c.hit1 > b.hit1 + HIT1_MARGIN_PP,
            "[{split_label}] personalized `{challenger}` hit@1 {:.3}% must beat baseline \
             `{base}` {:.3}% by > {HIT1_MARGIN_PP} pp",
            c.hit1,
            b.hit1,
        );
        assert!(
            c.mrr5 > b.mrr5 + MRR5_MARGIN,
            "[{split_label}] personalized `{challenger}` MRR@5 {:.3} must beat baseline \
             `{base}` {:.3} by > {MRR5_MARGIN}",
            c.mrr5,
            b.mrr5,
        );
    }
}

/// Train + evaluate the real pipeline for one split, returning the parsed report.
fn run_split(input: &Path, split: NextAppSplit, dir: &Path, tag: &str) -> Value {
    let artifact = dir.join(format!("artifact-{tag}.json"));
    let report = dir.join(format!("report-{tag}.json"));

    train(TrainOptions {
        dataset: NextAppDataset::Lsapp,
        input: input.to_path_buf(),
        output: artifact.clone(),
        horizon_secs: HORIZON_SECS,
        history_len: HISTORY_LEN,
        split,
    })
    .unwrap_or_else(|err| panic!("[{tag}] train failed: {err:#}"));

    evaluate(EvalOptions {
        dataset: NextAppDataset::Lsapp,
        input: input.to_path_buf(),
        artifact,
        output: report.clone(),
        horizon_secs: HORIZON_SECS,
        history_len: HISTORY_LEN,
        split,
    })
    .unwrap_or_else(|err| panic!("[{tag}] eval failed: {err:#}"));

    let value: Value = serde_json::from_reader(
        std::fs::File::open(&report).unwrap_or_else(|err| panic!("[{tag}] open report: {err}")),
    )
    .unwrap_or_else(|err| panic!("[{tag}] parse report: {err}"));
    assert_eq!(
        value["schema_version"], "dipecs.next_app_eval.v1",
        "[{tag}] unexpected report schema"
    );
    value
}

fn print_table(split_label: &str, report: &Value) {
    eprintln!(
        "\n=== {split_label} — hit@1% / MRR@5 (test_examples={}) ===",
        report["test_examples"]
    );
    for ranker in [
        "global_popularity",
        "mfu",
        "mru",
        "naive_bayes",
        "markov",
        "xgboost",
        "ensemble",
    ] {
        let m = metrics_of(report, ranker);
        eprintln!(
            "  {ranker:<18} hit@1 {:>6.3}%   MRR@5 {:.3}",
            m.hit1, m.mrr5
        );
    }
}

#[test]
fn personalized_models_beat_popularity_on_lsapp() {
    // --- Gate 1: debug builds are ~5x slower; steer to the release invocation.
    if cfg!(debug_assertions) && std::env::var_os("DIPECS_NEXT_APP_EVAL_FORCE").is_none() {
        eprintln!(
            "\n############################################################\n\
             # SKIPPED next_app_lsapp_test: debug build.                #\n\
             # Full LSApp train+eval is ~2min/split in debug.           #\n\
             # Run the release invocation instead:                      #\n\
             #   cargo test -p aios-cli --release \\                     #\n\
             #     --test next_app_lsapp_test -- --nocapture            #\n\
             # or force debug with DIPECS_NEXT_APP_EVAL_FORCE=1.        #\n\
             ############################################################\n"
        );
        return;
    }

    // --- Gate 2: the prepared LSApp trace is git-ignored (157 MB). If it is
    // absent, skip LOUDLY and return — never a silent pass.
    let input = lsapp_path();
    if !input.is_file() {
        eprintln!(
            "\n############################################################\n\
             # SKIPPED next_app_lsapp_test: LSApp fixture not found.    #\n\
             #   expected: {}\n\
             # Prepare it with:                                         #\n\
             #   git submodule update --init third_party/LSApp         #\n\
             #   bash tools/prepare-lsapp.sh                            #\n\
             # This is a documented conditional skip, not a pass.       #\n\
             ############################################################\n",
            input.display()
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!(
        "dipecs-next-app-lsapp-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");

    // ---------------------------------------------------------------------
    // Standard split: assert `ensemble` AND `markov` each strictly beat BOTH
    // popularity baselines on hit@1 and MRR@5.
    // ---------------------------------------------------------------------
    let standard = run_split(&input, NextAppSplit::Standard, &dir, "standard");
    print_table("standard split", &standard);
    assert_beats_all(&standard, "ensemble", POPULARITY_BASELINES, "standard");
    assert_beats_all(&standard, "markov", POPULARITY_BASELINES, "standard");

    // ---------------------------------------------------------------------
    // Cold-start split (held-out users): assert the BEST of {markov, xgboost}
    // strictly beats BOTH popularity baselines on hit@1 and MRR@5. We do NOT
    // require `ensemble` or `naive_bayes` here — both collapse below popularity
    // on unseen users, and asserting on them would be dishonest.
    // ---------------------------------------------------------------------
    let cold = run_split(&input, NextAppSplit::ColdStart, &dir, "cold-start");
    print_table("cold-start split", &cold);

    let cold_candidates = ["markov", "xgboost"];
    let cold_winners: Vec<&str> = cold_candidates
        .into_iter()
        .filter(|model| beats_all(&cold, model, POPULARITY_BASELINES))
        .collect();
    assert!(
        !cold_winners.is_empty(),
        "[cold-start] neither markov nor xgboost beat BOTH popularity baselines by margin \
         (hit@1 > +{HIT1_MARGIN_PP} pp AND MRR@5 > +{MRR5_MARGIN}). measured: \
         global_popularity hit@1 {:.3} MRR@5 {:.3}; mfu hit@1 {:.3} MRR@5 {:.3}; \
         markov hit@1 {:.3} MRR@5 {:.3}; xgboost hit@1 {:.3} MRR@5 {:.3}",
        metrics_of(&cold, "global_popularity").hit1,
        metrics_of(&cold, "global_popularity").mrr5,
        metrics_of(&cold, "mfu").hit1,
        metrics_of(&cold, "mfu").mrr5,
        metrics_of(&cold, "markov").hit1,
        metrics_of(&cold, "markov").mrr5,
        metrics_of(&cold, "xgboost").hit1,
        metrics_of(&cold, "xgboost").mrr5,
    );
    eprintln!(
        "\ncold-start personalized winners over popularity: {}",
        cold_winners.join(", ")
    );

    // Tidy up the temp artifacts/reports on success.
    let _ = std::fs::remove_dir_all(&dir);

    eprintln!(
        "\nOK: personalized models beat popularity baselines on both LSApp splits \
         (margins: hit@1 > +{HIT1_MARGIN_PP} pp, MRR@5 > +{MRR5_MARGIN})."
    );
}
