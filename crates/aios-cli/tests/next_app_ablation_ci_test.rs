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
fn bench_workflow_runs_lsapp_ablation_as_release_label_gated_lane() {
    let workflow_path = repo_root().join(".github/workflows/bench.yml");
    let workflow = fs::read_to_string(&workflow_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", workflow_path.display()));
    let action_benefit_job = action_benefit_job(&workflow);

    assert!(
        workflow.contains("schedule:") && workflow.contains("cron:"),
        "bench workflow must include a scheduled trigger for the heavy LSApp gate"
    );
    assert!(
        workflow.contains("types:") && workflow.contains("labeled"),
        "pull_request trigger must include labeled so next-app-eval can start the gate"
    );
    assert!(
        action_benefit_job.contains("LSApp Personalization Ablation (optional)"),
        "bench workflow must keep the LSApp personalization ablation lane"
    );
    assert!(
        action_benefit_job.contains("github.event_name == 'schedule'")
            && action_benefit_job.contains("next-app-eval"),
        "LSApp ablation must stay scheduled or next-app-eval label gated"
    );
    assert!(
        action_benefit_job.contains("./tools/prepare-lsapp.sh"),
        "LSApp ablation lane must prepare the git-ignored fixture"
    );
    assert!(
        action_benefit_job.contains("submodules: true")
            || action_benefit_job.contains("submodules: recursive"),
        "checkout must fetch the LSApp submodule before prepare-lsapp runs"
    );
    assert!(
        action_benefit_job
            .contains("cargo test -j 1 -p aios-cli --release --test next_app_ablation_test"),
        "LSApp ablation must run in release mode with one cargo job to bound runner pressure"
    );
    assert!(
        action_benefit_job.contains("personalization_contribution_on_lsapp -- --nocapture"),
        "test filter must come before test-harness args so --release remains a cargo arg"
    );
    assert!(
        !action_benefit_job.contains("-- personalization_contribution_on_lsapp"),
        "regression guard: do not put the test filter after cargo's -- separator"
    );
}
