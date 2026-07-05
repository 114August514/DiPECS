const SCRIPT: &str =
    include_str!("../../../../tools/collect/collect-release-memory-pressure-benefit.sh");

#[test]
fn release_memory_collection_script_fails_closed_on_missing_pressure_evidence() {
    assert!(
        SCRIPT.contains("SAMPLES must be >=20"),
        "ReleaseMemory pressure evidence collection must enforce n>=20"
    );
    assert!(
        SCRIPT.contains("PRESSURE_COMMAND is required for #99 evidence"),
        "ReleaseMemory pressure collection must require an explicit pressure command"
    );
    assert!(
        SCRIPT.contains("start_pressure_window") && SCRIPT.contains("finish_pressure_window"),
        "ReleaseMemory pressure collection must keep the pressure source alive across before/after measurements"
    );
    assert!(
        SCRIPT.contains("exited before the post-pressure measurement window"),
        "ReleaseMemory pressure collection must fail when pressure exits before action sampling"
    );
    assert!(
        SCRIPT.contains("ReleaseMemory missing device response"),
        "ReleaseMemory collection must fail when the bridge response cannot be parsed"
    );
    assert!(
        SCRIPT.contains("ReleaseMemory bridge did not accept action"),
        "ReleaseMemory collection must require an ok bridge response"
    );
    assert!(
        SCRIPT.contains("memory_pressure_observed"),
        "ReleaseMemory artifacts must record whether pressure was observed"
    );
    assert!(
        SCRIPT.contains("available_gain_kb") && SCRIPT.contains("pss_reduction_gain_kb"),
        "ReleaseMemory artifacts must report available memory and PSS effects"
    );
    assert!(
        SCRIPT.contains("jank_delta_vs_baseline_pct_points"),
        "ReleaseMemory artifacts must report jank regression or improvement"
    );
    assert!(
        SCRIPT.contains("release_memory_effective"),
        "ReleaseMemory acceptance must require an explicit effect gate"
    );
    assert!(
        !SCRIPT.contains("\"accepted\": True"),
        "ReleaseMemory artifact acceptance must not be hard-coded"
    );
    assert!(
        SCRIPT.contains("\"accepted\": accepted"),
        "ReleaseMemory artifact acceptance must be derived from measured gates"
    );
    assert!(
        SCRIPT.contains("statistically_significant"),
        "ReleaseMemory acceptance must include a statistical significance gate"
    );
    assert!(
        SCRIPT.contains("welch_t_test"),
        "ReleaseMemory must use Welch's t-test for significance testing"
    );
    assert!(
        SCRIPT.contains("p_value"),
        "ReleaseMemory artifacts must report p-values for each metric"
    );
    assert!(
        SCRIPT.contains("significance_alpha"),
        "ReleaseMemory must define a significance threshold"
    );
    assert!(
        SCRIPT.contains("available_memory_significant"),
        "ReleaseMemory acceptance must require significant available-memory gain"
    );
    assert!(
        SCRIPT.contains("measured_no_significant_benefit"),
        "ReleaseMemory artifacts must distinguish measured negative results from missing pressure evidence"
    );
    assert!(
        SCRIPT.contains("pss_non_regression_required")
            && SCRIPT.contains("jank_non_regression_required"),
        "ReleaseMemory acceptance should treat PSS and jank as non-regression gates"
    );
    assert!(
        SCRIPT.contains("math.inf if mean_a > mean_b else -math.inf"),
        "ReleaseMemory Welch test must handle zero-variance unequal means"
    );
}
