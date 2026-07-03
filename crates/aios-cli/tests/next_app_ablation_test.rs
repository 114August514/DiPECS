//! PERSONALIZATION ABLATION for the next-app prediction system, computed from
//! the real 3.66M-row LSApp trace. This backs the paper's ablation section, so
//! every number is trained and evaluated fresh here — nothing is read from the
//! committed `data/evaluation/lsapp-*.report.json` (that would make the test a
//! change-detector rather than a real measurement).
//!
//! We quantify "how much does per-user personalization contribute?" with two
//! complementary, honest measurements — each an assertion on a *relationship*
//! (never a hardcoded metric value), plus an ablation TABLE printed to stderr
//! for lifting into the paper.
//!
//! ## What is "personalized" here
//! Two models genuinely key on the user:
//!   * `markov`   — `rank_markov` looks up a `user_id`-keyed transition table
//!     first (`user_transition_key(user, current)`), only falling back to the
//!     global transition table when that user/context is unseen.
//!   * `ensemble` — weights `markov` at 0.40 alongside naive-bayes / xgboost.
//!
//! The non-personalized control is `global_popularity`, which ignores the user
//! entirely and ranks by global label frequency.
//!
//! ## Measurement 1 — personalization ON vs OFF, within the STANDARD split
//! Personalized models (`markov`, `ensemble`) vs non-personalized
//! `global_popularity`, all on the same standard split (per-user history is
//! available). Primary metric is `macro_hit_rate_at_1_pct` (per-user averaged,
//! so every user counts equally regardless of trace length); we also assert on
//! micro `hit_rate_at_1_pct`. Personalization must strictly help by a margin.
//!
//! ## Measurement 2 — personalization AVAILABILITY ablation, via split
//! The SAME personalized models on standard (per-user history available) vs
//! cold-start (entirely held-out users — their per-user transition rows do not
//! exist, so `rank_markov` falls back to global). Removing per-user history
//! must strictly degrade them; the standard−coldstart gap IS the personalization
//! contribution in pp. As a specificity control we also measure
//! `global_popularity`: it never used the user, so it does NOT benefit — its
//! standard−coldstart gap is ~0/reversed, and we assert each personalized
//! model's gap strictly exceeds that control gap.
//!
//! ## Runtime / how to run
//! Full train+eval per split is ~24s in release, ~2min in debug; this runs two
//! splits. Canonical invocation is release:
//!   cargo test -p aios-cli --release --test next_app_ablation_test -- --nocapture
//! In a debug build the test skips LOUDLY (pointing at `--release`) unless
//! `DIPECS_NEXT_APP_EVAL_FORCE=1` is set. Never a silent pass.
//!
//! ## Data availability
//! `data/lsapp/lsapp.tsv` is git-ignored (157 MB) and only exists after
//! `git submodule update --init third_party/LSApp && bash tools/prepare-lsapp.sh`.
//! When it is absent the test skips LOUDLY and returns — a runner without the
//! fixture cannot manufacture a fake green.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aios_cli::next_app::{
    evaluate, train, EvalOptions, NextAppDataset, NextAppSplit, TrainOptions,
};
use serde_json::Value;

/// Match the CLI defaults used to generate the committed reports, so this test
/// reproduces the same measurement the paper cites.
const HORIZON_SECS: u64 = 30;
const HISTORY_LEN: usize = 5;

/// The user-personalized models (they consult a `user_id`-keyed table) and the
/// non-personalized control (ignores the user entirely).
const PERSONALIZED_MODELS: &[&str] = &["markov", "ensemble"];
const NON_PERSONALIZED: &str = "global_popularity";

/// M1 margin: minimum strict pp by which each personalized model must beat the
/// non-personalized `global_popularity` on BOTH macro-hit@1 and micro-hit@1,
/// within the standard split. Chosen below HALF the smallest measured full-LSApp
/// delta so the claim keeps headroom against minor nondeterminism.
///
/// Measured deltas (personalized − global_popularity, standard split; committed
/// reports):
///   * markov:   +21.09 pp macro, +25.67 pp micro-hit@1
///   * ensemble: +19.83 pp macro, +27.73 pp micro-hit@1
///
/// Smallest is 19.83 pp (ensemble macro); half is ~9.9 pp, so 8.0 pp sits below
/// half with ~2.5x headroom to the real delta.
const PERSONALIZATION_MARGIN_PP: f64 = 8.0;

/// M2 margin: minimum strict pp by which each personalized model must score
/// HIGHER on the standard split than on cold-start (i.e. removing per-user
/// history degrades it), on BOTH macro-hit@1 and micro-hit@1.
///
/// Measured deltas (standard − coldstart, personalized; committed reports):
///   * markov:   +14.58 pp micro, +14.26 pp macro
///   * ensemble: +17.47 pp micro, +16.07 pp macro
///
/// Smallest is 14.26 pp (markov macro); half is ~7.1 pp, so 6.0 pp sits below
/// half with ~2.4x headroom.
const AVAILABILITY_MARGIN_PP: f64 = 6.0;

/// Control-specificity margin: each personalized model's standard−coldstart gap
/// must exceed the non-personalized control's own standard−coldstart gap by at
/// least this much (on both metrics), proving the availability effect is
/// specific to personalization rather than a split artifact.
///
/// Measured control gap (global_popularity, standard − coldstart) is negative:
/// −3.17 pp micro, −0.39 pp macro. So personalized gap − control gap is at least
/// 14.58−(−3.17)=17.75 pp (markov micro) / 14.26−(−0.39)=14.65 pp (markov macro);
/// smallest 14.65 pp, half ~7.3 pp, so 6.0 pp keeps ~2.4x headroom.
const CONTROL_MARGIN_PP: f64 = 6.0;

fn lsapp_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/lsapp/lsapp.tsv")
}

/// The two hit@1 flavours this ablation compares: micro (example-weighted) and
/// macro (per-user averaged — the primary personalization metric).
#[derive(Clone, Copy)]
struct Metrics {
    micro_hit1: f64,
    macro_hit1: f64,
}

fn metrics_of(report: &Value, ranker: &str) -> Metrics {
    let node = &report["metrics"][ranker];
    Metrics {
        micro_hit1: node["hit_rate_at_1_pct"]
            .as_f64()
            .unwrap_or_else(|| panic!("report missing hit_rate_at_1_pct for `{ranker}`")),
        macro_hit1: node["macro_hit_rate_at_1_pct"]
            .as_f64()
            .unwrap_or_else(|| panic!("report missing macro_hit_rate_at_1_pct for `{ranker}`")),
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

#[test]
fn personalization_contribution_on_lsapp() {
    // --- Gate 1: debug builds are ~5x slower; steer to the release invocation.
    if cfg!(debug_assertions) && std::env::var_os("DIPECS_NEXT_APP_EVAL_FORCE").is_none() {
        eprintln!(
            "\n############################################################\n\
             # SKIPPED next_app_ablation_test: debug build.             #\n\
             # Full LSApp train+eval is ~2min/split in debug (x2).      #\n\
             # Run the release invocation instead:                      #\n\
             #   cargo test -p aios-cli --release \\                     #\n\
             #     --test next_app_ablation_test -- --nocapture         #\n\
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
             # SKIPPED next_app_ablation_test: LSApp fixture not found. #\n\
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
        "dipecs-next-app-ablation-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");

    // Two real train+eval runs: personalization is available in `standard`
    // (per-user history) and unavailable in `cold-start` (held-out users).
    let standard = run_split(&input, NextAppSplit::Standard, &dir, "standard");
    let cold = run_split(&input, NextAppSplit::ColdStart, &dir, "cold-start");

    // =====================================================================
    // Measurement 1: personalization ON vs OFF within the STANDARD split.
    // Each personalized model must strictly beat the non-personalized
    // `global_popularity` on BOTH macro-hit@1 (primary) and micro-hit@1.
    // =====================================================================
    let off = metrics_of(&standard, NON_PERSONALIZED);
    eprintln!("\n==================== PERSONALIZATION ABLATION (LSApp) ====================");
    eprintln!(
        "\n[M1] Personalization ON vs OFF within STANDARD split \
         (standard test_examples={})",
        standard["test_examples"]
    );
    eprintln!("     primary metric = macro_hit_rate_at_1_pct (per-user averaged)");
    eprintln!(
        "     OFF  {NON_PERSONALIZED:<17}  macro {:>6.3}%   micro-hit@1 {:>6.3}%",
        off.macro_hit1, off.micro_hit1
    );
    eprintln!("     ON  (personalized)     macro%   Δmacro(pp)   micro%   Δmicro(pp)");
    let mut m1_macro_deltas: Vec<f64> = Vec::new();
    for &model in PERSONALIZED_MODELS {
        let on = metrics_of(&standard, model);
        let d_macro = on.macro_hit1 - off.macro_hit1;
        let d_micro = on.micro_hit1 - off.micro_hit1;
        eprintln!(
            "         {model:<17}  {:>6.3}   {:>+7.3}     {:>6.3}   {:>+7.3}",
            on.macro_hit1, d_macro, on.micro_hit1, d_micro
        );
        assert!(
            on.macro_hit1 > off.macro_hit1 + PERSONALIZATION_MARGIN_PP,
            "[M1] personalized `{model}` macro-hit@1 {:.3}% must beat non-personalized \
             `{NON_PERSONALIZED}` {:.3}% by > {PERSONALIZATION_MARGIN_PP} pp",
            on.macro_hit1,
            off.macro_hit1,
        );
        assert!(
            on.micro_hit1 > off.micro_hit1 + PERSONALIZATION_MARGIN_PP,
            "[M1] personalized `{model}` micro-hit@1 {:.3}% must beat non-personalized \
             `{NON_PERSONALIZED}` {:.3}% by > {PERSONALIZATION_MARGIN_PP} pp",
            on.micro_hit1,
            off.micro_hit1,
        );
        m1_macro_deltas.push(d_macro);
    }
    let (m1_lo, m1_hi) = min_max(&m1_macro_deltas);
    eprintln!("     -> personalization contributes +{m1_lo:.3}..+{m1_hi:.3} pp macro-hit@1");

    // =====================================================================
    // Measurement 2: personalization AVAILABILITY ablation via split.
    // SAME personalized models on standard (history available) vs cold-start
    // (unseen users, no per-user history). Each must score strictly HIGHER on
    // standard, on BOTH metrics. The standard−coldstart gap is the contribution.
    //
    // Control: `global_popularity` never used the user, so its own gap is
    // ~0/reversed; assert each personalized gap strictly exceeds the control gap
    // (specificity — the effect is about personalization, not the split).
    // =====================================================================
    let ctrl_std = metrics_of(&standard, NON_PERSONALIZED);
    let ctrl_cold = metrics_of(&cold, NON_PERSONALIZED);
    let ctrl_gap_micro = ctrl_std.micro_hit1 - ctrl_cold.micro_hit1;
    let ctrl_gap_macro = ctrl_std.macro_hit1 - ctrl_cold.macro_hit1;

    eprintln!(
        "\n[M2] Personalization AVAILABILITY: STANDARD vs COLD-START \
         (cold-start test_examples={})",
        cold["test_examples"]
    );
    eprintln!("     removing per-user history (held-out users) should degrade personalized models");
    eprintln!(
        "     model              std-micro%  cold-micro%  Δ(pp)     std-macro%  cold-macro%  Δ(pp)"
    );
    let mut m2_micro_deltas: Vec<f64> = Vec::new();
    for &model in PERSONALIZED_MODELS {
        let s = metrics_of(&standard, model);
        let c = metrics_of(&cold, model);
        let gap_micro = s.micro_hit1 - c.micro_hit1;
        let gap_macro = s.macro_hit1 - c.macro_hit1;
        eprintln!(
            "     {model:<17}  {:>9.3}  {:>10.3}  {:>+7.3}   {:>9.3}  {:>10.3}  {:>+7.3}",
            s.micro_hit1, c.micro_hit1, gap_micro, s.macro_hit1, c.macro_hit1, gap_macro
        );

        // Availability: standard strictly beats cold-start on both metrics.
        assert!(
            gap_micro > AVAILABILITY_MARGIN_PP,
            "[M2] personalized `{model}` micro-hit@1 should drop >= {AVAILABILITY_MARGIN_PP} pp \
             when per-user history is removed (standard {:.3}% -> cold-start {:.3}%, gap {gap_micro:.3} pp)",
            s.micro_hit1,
            c.micro_hit1,
        );
        assert!(
            gap_macro > AVAILABILITY_MARGIN_PP,
            "[M2] personalized `{model}` macro-hit@1 should drop >= {AVAILABILITY_MARGIN_PP} pp \
             when per-user history is removed (standard {:.3}% -> cold-start {:.3}%, gap {gap_macro:.3} pp)",
            s.macro_hit1,
            c.macro_hit1,
        );

        // Specificity control: the personalized gap must exceed the
        // non-personalized control's own standard−coldstart gap by a margin.
        assert!(
            gap_micro > ctrl_gap_micro + CONTROL_MARGIN_PP,
            "[M2-control] personalized `{model}` micro gap {gap_micro:.3} pp must exceed \
             non-personalized `{NON_PERSONALIZED}` gap {ctrl_gap_micro:.3} pp by > {CONTROL_MARGIN_PP} pp \
             (availability effect must be personalization-specific)",
        );
        assert!(
            gap_macro > ctrl_gap_macro + CONTROL_MARGIN_PP,
            "[M2-control] personalized `{model}` macro gap {gap_macro:.3} pp must exceed \
             non-personalized `{NON_PERSONALIZED}` gap {ctrl_gap_macro:.3} pp by > {CONTROL_MARGIN_PP} pp \
             (availability effect must be personalization-specific)",
        );
        m2_micro_deltas.push(gap_micro);
    }
    eprintln!(
        "     CONTROL {NON_PERSONALIZED:<10}  {:>9.3}  {:>10.3}  {:>+7.3}   {:>9.3}  {:>10.3}  {:>+7.3}",
        ctrl_std.micro_hit1,
        ctrl_cold.micro_hit1,
        ctrl_gap_micro,
        ctrl_std.macro_hit1,
        ctrl_cold.macro_hit1,
        ctrl_gap_macro,
    );
    let (m2_lo, m2_hi) = min_max(&m2_micro_deltas);
    eprintln!(
        "     -> personalization availability contributes +{m2_lo:.3}..+{m2_hi:.3} pp micro-hit@1"
    );
    eprintln!(
        "        (control {NON_PERSONALIZED} gap {ctrl_gap_micro:+.3} pp micro / {ctrl_gap_macro:+.3} pp \
         macro: non-personalized does NOT benefit -> effect is personalization-specific)"
    );
    eprintln!("=========================================================================\n");

    // Tidy up temp artifacts/reports on success.
    let _ = std::fs::remove_dir_all(&dir);

    eprintln!(
        "OK: personalization contributes +{m1_lo:.1}..+{m1_hi:.1} pp macro-hit@1 (ON vs OFF, standard) \
         and +{m2_lo:.1}..+{m2_hi:.1} pp micro-hit@1 (availability: standard vs cold-start)."
    );
}

/// Smallest and largest of a slice of deltas. Returns (0.0, 0.0) for an empty
/// slice so a future caller cannot panic; all current call sites pass non-empty
/// slices.
fn min_max(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let mut lo = values[0];
    let mut hi = values[0];
    for &v in &values[1..] {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    (lo, hi)
}
