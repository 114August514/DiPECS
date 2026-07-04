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
//! Two models genuinely key on the user, but to different degrees:
//!   * `markov`   — PURE personalization: `rank_markov` looks up a `user_id`-keyed
//!     transition table first (`user_transition_key(user, current)`), only falling
//!     back to the global transition table when that user/context is unseen.
//!   * `ensemble` — RRF fusion (`rank_ensemble_rrf`) that blends `markov` with
//!     GLOBAL components (naive_bayes, feature_lift) via a learned
//!     reciprocal-rank-fusion combiner fit on a held-out slice. Because two of its
//!     three components never use the user, it is BY DESIGN more cold-start robust
//!     than pure markov — it degrades far less when per-user history is removed.
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
//! exist, so `rank_markov` falls back to global). Assertions, honest about each
//! model's design:
//!   * BOTH models must strictly degrade (standard > cold-start on both metrics)
//!     and each must strictly exceed the non-personalized control's own gap
//!     (specificity — the effect is personalization, not a split artifact).
//!   * The PURE model `markov` must degrade by a STRONG margin
//!     (`MARKOV_AVAILABILITY_MARGIN_PP`). The RRF `ensemble` is intentionally NOT
//!     held to that margin — its global components cushion the loss.
//!   * Fusion robustness (the positive claim): `markov`'s drop must EXCEED
//!     `ensemble`'s drop by `FUSION_ROBUSTNESS_MARGIN_PP`, quantifying exactly how
//!     much cold-start robustness the fusion buys.
//!
//! ## Runtime / how to run
//! Full train+eval over LSApp is intentionally heavy (release冷编译约 55 分钟 +
//! two train+eval passes) and is LOCAL-ONLY — it is `#[ignore]`d so plain
//! `cargo test` skips it, and it is NOT run in CI (see `.github/workflows/bench.yml`).
//! Canonical local invocation is release with one cargo job:
//!   cargo test -j 1 -p aios-cli --release --test next_app_ablation_test \
//!     -- --ignored --nocapture
//! Avoid running it from constrained IDE sessions unless the machine has spare RAM.
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
/// Measured deltas (personalized − global_popularity, standard split; current
/// RRF ensemble on full LSApp):
///   * markov:   +21.29 pp macro, +26.04 pp micro-hit@1
///   * ensemble: +34.95 pp macro, +42.92 pp micro-hit@1
///
/// Smallest is 21.29 pp (markov macro); half is ~10.6 pp, so 8.0 pp sits below
/// half with ample headroom to the real delta.
const PERSONALIZATION_MARGIN_PP: f64 = 8.0;

/// M2 margin for the PURE-personalization model (`markov`): it consults only a
/// `user_id`-keyed transition table (falling back to global when the user is
/// unseen), so removing per-user history must degrade it by a STRONG margin on
/// BOTH macro-hit@1 and micro-hit@1.
///
/// Measured drop (standard − coldstart, `markov`): +21.72 pp micro, +15.15 pp
/// macro. Smallest is 15.15 pp (macro); half is ~7.5 pp, so 6.0 pp keeps ~2.5x
/// headroom.
///
/// NOTE: this margin is deliberately NOT applied to `ensemble`. The RRF
/// `ensemble` blends `markov` with GLOBAL components (naive_bayes, feature_lift)
/// that never used the user, so it is BY DESIGN more cold-start robust and
/// degrades far less (see [`FUSION_ROBUSTNESS_MARGIN_PP`]). Requiring ensemble
/// to also drop 6 pp would encode a false premise about how it works.
const MARKOV_AVAILABILITY_MARGIN_PP: f64 = 6.0;

/// Fusion-robustness margin: the pure `markov` model's standard−coldstart drop
/// must EXCEED the RRF `ensemble`'s drop by at least this much, on BOTH metrics.
/// This is the honest positive claim about the ensemble — its global components
/// buy cold-start robustness, so a personalization-availability shock hurts the
/// pure model much more than the fused one.
///
/// Measured drops: markov 21.72/15.15 pp vs ensemble 6.06/3.90 pp; the smaller
/// separation is 15.15−3.90 = 11.25 pp (macro), half ~5.6 pp, so 4.0 pp keeps
/// headroom.
const FUSION_ROBUSTNESS_MARGIN_PP: f64 = 4.0;

/// Specificity floor: each personalized model's standard−coldstart gap must
/// STRICTLY EXCEED the non-personalized control's own gap on both metrics,
/// proving the availability effect is personalization-specific and not a split
/// artifact. The control (`global_popularity`) never used the user, so its gap
/// is ~0/negative (measured −3.17 pp micro, −0.39 pp macro). A strict-exceed
/// (margin 0) is the right floor here: even the fusion-robust ensemble
/// (macro gap +3.90) clears the control, while a mere split artifact would not.
const CONTROL_MARGIN_PP: f64 = 0.0;

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

// LOCAL-ONLY: `#[ignore]` so plain `cargo test` and CI skip this heavy gate.
// Run it explicitly with `--ignored` (see module header for the canonical
// release invocation). It is deliberately absent from CI (bench.yml).
#[test]
#[ignore = "heavy local-only LSApp train+eval; run with --ignored (see module header)"]
fn personalization_contribution_on_lsapp() {
    // --- Gate 1: debug builds are ~5x slower; steer to the release invocation.
    if cfg!(debug_assertions) && std::env::var_os("DIPECS_NEXT_APP_EVAL_FORCE").is_none() {
        eprintln!(
            "\n############################################################\n\
             # SKIPPED next_app_ablation_test: debug build.             #\n\
             # Full LSApp train+eval is ~2min/split in debug (x2).      #\n\
             # Run the release invocation instead:                      #\n\
             #   cargo test -j 1 -p aios-cli --release \\                #\n\
             #     --test next_app_ablation_test \\                      #\n\
             #     personalization_contribution_on_lsapp -- --nocapture #\n\
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
    // Per-model standard−coldstart gaps, keyed by model, for the cross-model
    // fusion-robustness assertion after the loop.
    let mut gaps_micro: std::collections::BTreeMap<&str, f64> = std::collections::BTreeMap::new();
    let mut gaps_macro: std::collections::BTreeMap<&str, f64> = std::collections::BTreeMap::new();
    for &model in PERSONALIZED_MODELS {
        let s = metrics_of(&standard, model);
        let c = metrics_of(&cold, model);
        let gap_micro = s.micro_hit1 - c.micro_hit1;
        let gap_macro = s.macro_hit1 - c.macro_hit1;
        eprintln!(
            "     {model:<17}  {:>9.3}  {:>10.3}  {:>+7.3}   {:>9.3}  {:>10.3}  {:>+7.3}",
            s.micro_hit1, c.micro_hit1, gap_micro, s.macro_hit1, c.macro_hit1, gap_macro
        );
        gaps_micro.insert(model, gap_micro);
        gaps_macro.insert(model, gap_macro);

        // Every personalized model must degrade at least a little when per-user
        // history is removed — standard strictly beats cold-start on both
        // metrics. This is the universal availability direction.
        assert!(
            gap_micro > 0.0,
            "[M2] personalized `{model}` micro-hit@1 must drop when per-user history is \
             removed (standard {:.3}% -> cold-start {:.3}%, gap {gap_micro:.3} pp)",
            s.micro_hit1,
            c.micro_hit1,
        );
        assert!(
            gap_macro > 0.0,
            "[M2] personalized `{model}` macro-hit@1 must drop when per-user history is \
             removed (standard {:.3}% -> cold-start {:.3}%, gap {gap_macro:.3} pp)",
            s.macro_hit1,
            c.macro_hit1,
        );

        // Specificity floor: the personalized gap must strictly exceed the
        // non-personalized control's own standard−coldstart gap on both metrics,
        // proving the availability effect is personalization-specific and not a
        // split artifact. (Control gap is ~0/negative; margin 0 = strict-exceed.)
        assert!(
            gap_micro > ctrl_gap_micro + CONTROL_MARGIN_PP,
            "[M2-control] personalized `{model}` micro gap {gap_micro:.3} pp must exceed \
             non-personalized `{NON_PERSONALIZED}` gap {ctrl_gap_micro:.3} pp \
             (availability effect must be personalization-specific)",
        );
        assert!(
            gap_macro > ctrl_gap_macro + CONTROL_MARGIN_PP,
            "[M2-control] personalized `{model}` macro gap {gap_macro:.3} pp must exceed \
             non-personalized `{NON_PERSONALIZED}` gap {ctrl_gap_macro:.3} pp \
             (availability effect must be personalization-specific)",
        );

        // Model-specific strength: `markov` is a PURE personalization model, so
        // removing per-user history must hurt it by a STRONG margin. `ensemble`
        // is intentionally exempt — its global RRF components (naive_bayes /
        // feature_lift) make it cold-start robust; that robustness is asserted
        // as a positive claim below, not penalized here.
        if model == "markov" {
            assert!(
                gap_micro > MARKOV_AVAILABILITY_MARGIN_PP
                    && gap_macro > MARKOV_AVAILABILITY_MARGIN_PP,
                "[M2] pure-personalization `markov` should drop >= {MARKOV_AVAILABILITY_MARGIN_PP} pp \
                 on both metrics when per-user history is removed (micro gap {gap_micro:.3} pp, \
                 macro gap {gap_macro:.3} pp)",
            );
        }

        m2_micro_deltas.push(gap_micro);
    }

    // Fusion robustness (the honest positive claim): the pure `markov` model is
    // hurt by the personalization-availability shock MORE than the RRF
    // `ensemble`, because ensemble's global components cushion the loss. Assert
    // markov's drop exceeds ensemble's drop by a margin, on both metrics.
    let markov_micro = gaps_micro["markov"];
    let markov_macro = gaps_macro["markov"];
    let ensemble_micro = gaps_micro["ensemble"];
    let ensemble_macro = gaps_macro["ensemble"];
    assert!(
        markov_micro > ensemble_micro + FUSION_ROBUSTNESS_MARGIN_PP
            && markov_macro > ensemble_macro + FUSION_ROBUSTNESS_MARGIN_PP,
        "[M2-fusion] pure `markov` should lose more to cold-start than RRF `ensemble` by \
         > {FUSION_ROBUSTNESS_MARGIN_PP} pp (markov drop {markov_micro:.3}/{markov_macro:.3} pp \
         micro/macro vs ensemble {ensemble_micro:.3}/{ensemble_macro:.3} pp): ensemble's global \
         components should cushion the loss of per-user history",
    );
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
    eprintln!(
        "     -> fusion robustness: pure markov loses {markov_macro:.3} pp macro to cold-start, \
         RRF ensemble only {ensemble_macro:.3} pp (global components cushion per-user loss)"
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
