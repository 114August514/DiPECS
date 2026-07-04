//! Structural guard for `.github/workflows/bench.yml`.
//!
//! The heavy full-LSApp train+eval ablation is intentionally NOT run in CI: it
//! cold-compiles the release dependency chain (~55 min) and belongs in the
//! local `#[ignore]` test `next_app_ablation_test.rs`. This guard pins that
//! decision so a future edit cannot silently re-introduce the多分钟 gate into
//! every PR. It also pins the `timeout-minutes` backstop that keeps a hung step
//! from occupying a runner for the 6-hour default.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn action_benefit_job(workflow: &str) -> &str {
    workflow
        .split_once("  action-benefit-guard:")
        .map(|(_, section)| section)
        .expect("bench workflow must define action-benefit-guard")
}

#[test]
fn bench_workflow_keeps_heavy_lsapp_ablation_out_of_ci() {
    let workflow_path = repo_root().join(".github/workflows/bench.yml");
    let workflow = fs::read_to_string(&workflow_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", workflow_path.display()));
    let action_benefit_job = action_benefit_job(&workflow);

    // The heavy train+eval must NOT run in CI in any form.
    assert!(
        !workflow.contains("cargo test -j 1 -p aios-cli --release --test next_app_ablation_test"),
        "the heavy LSApp ablation train+eval must NOT run in CI (it is a local #[ignore] test); \
         see next_app_ablation_test.rs module header"
    );
    assert!(
        !action_benefit_job.contains("personalization_contribution_on_lsapp"),
        "the LSApp ablation test must not be invoked from CI"
    );
    assert!(
        !action_benefit_job.contains("./tools/prepare-lsapp.sh"),
        "CI should not prepare the 157 MB LSApp fixture for the removed ablation lane"
    );

    // The scheduled / label-gated triggers that only served the heavy gate are
    // gone — PRs get fast feedback and no one can opt a PR into the 55-min lane.
    assert!(
        !workflow.contains("schedule:") && !workflow.contains("cron:"),
        "the weekly scheduled heavy-gate trigger should be removed"
    );
    assert!(
        !workflow.contains("labeled"),
        "the next-app-eval label opt-in for the heavy gate should be removed"
    );

    // The job must carry a timeout backstop so a hung step fails fast instead of
    // running to GitHub's 6-hour default (this incident's root cause).
    assert!(
        action_benefit_job.contains("timeout-minutes:"),
        "action-benefit-guard must set timeout-minutes as a hung-step backstop"
    );

    // The lightweight guards that DO belong in CI must stay.
    assert!(
        action_benefit_job.contains("PreWarm UX Benefit Regression"),
        "the seconds-fast PreWarm UX regression must remain in CI"
    );
    assert!(
        action_benefit_job.contains("StrongPredictiveActionBaseline Report Guard"),
        "the StrongBaseline report smoke guard must remain in CI"
    );
}
