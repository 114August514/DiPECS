const SCRIPT: &str = include_str!("../../../../tools/collect/collect-prefetch-file-benefit.sh");

#[test]
fn prefetch_collection_script_fails_closed_on_missing_measurements() {
    assert!(
        SCRIPT.contains("SAMPLES must be >=20"),
        "PrefetchFile evidence collection must enforce n>=20"
    );
    assert!(
        SCRIPT.contains("missing device response"),
        "PrefetchFile collection must fail when the bridge response cannot be parsed"
    );
    assert!(
        SCRIPT.contains("bridge did not accept action"),
        "PrefetchFile collection must require an ok bridge response"
    );
    assert!(
        SCRIPT.contains("PrefetchFile did not create expected cache file"),
        "PrefetchFile collection must fail when the cache artifact is not created"
    );
    assert!(
        SCRIPT.contains("same_budget_baseline_inputs_present"),
        "PrefetchFile artifacts must record whether strong-baseline inputs were provided"
    );
    assert!(
        SCRIPT.contains("DIPECS_HIT_RATE_PCT") && SCRIPT.contains("STRONG_HIT_RATE_PCT"),
        "PrefetchFile net-benefit collection must support same-budget DiPECS vs strong baseline inputs"
    );
    assert!(
        SCRIPT.contains("send_action PrefetchFile"),
        "PrefetchFile collection must actually dispatch PrefetchFile actions"
    );
    assert!(
        !SCRIPT.contains("\"accepted\": True"),
        "PrefetchFile artifact acceptance must not be hard-coded"
    );
    assert!(
        SCRIPT.contains("\"accepted\": accepted"),
        "PrefetchFile artifact acceptance must be derived from measured gates"
    );
}

#[test]
fn prefetch_collection_script_preserves_run_as_shell_command_strings() {
    assert!(
        SCRIPT.contains("run_as_sh()"),
        "PrefetchFile collection must wrap run-as sh -c commands so adb shell receives one remote command string"
    );
    assert!(
        !SCRIPT.contains("adb_cmd shell run-as \"$PACKAGE\" sh -c"),
        "PrefetchFile collection must not pass run-as sh -c as split adb shell argv; Android adb shell drops the intended command string"
    );
}

#[test]
fn prefetch_collection_script_waits_for_stable_cache_size_before_reading() {
    assert!(
        SCRIPT.contains("CACHE_STABLE_POLLS"),
        "PrefetchFile collection must wait for the cache file size to stabilize before measuring reads"
    );
    assert!(
        SCRIPT.contains("stable_polls"),
        "PrefetchFile collection must require repeated stable size observations, not only a non-empty cache file"
    );
}
