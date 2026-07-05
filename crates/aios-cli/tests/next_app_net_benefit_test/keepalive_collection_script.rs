const SCRIPT: &str =
    include_str!("../../../../tools/collect/collect-keepalive-pressure-benefit.sh");

#[test]
fn keepalive_collection_script_fails_closed_on_missing_pressure_evidence() {
    assert!(
        SCRIPT.contains("SAMPLES must be >=20"),
        "KeepAlive pressure evidence collection must enforce n>=20"
    );
    assert!(
        SCRIPT.contains("PRESSURE_COMMAND is required for #98 evidence"),
        "KeepAlive pressure collection must require an explicit pressure command"
    );
    assert!(
        SCRIPT.contains("KeepAlive missing device response"),
        "KeepAlive collection must fail when the bridge response cannot be parsed"
    );
    assert!(
        SCRIPT.contains("KeepAlive bridge did not accept action"),
        "KeepAlive collection must require an ok bridge response"
    );
    assert!(
        SCRIPT.contains("memory_pressure_observed"),
        "KeepAlive artifacts must record whether pressure was observed"
    );
    assert!(
        SCRIPT.contains("survival_rate_pct") && SCRIPT.contains("restart_count_mean"),
        "KeepAlive artifacts must report process survival and restart metrics"
    );
    assert!(
        SCRIPT.contains("mean_jank_pct") && SCRIPT.contains("mean_pss_delta_kb"),
        "KeepAlive artifacts must report jank and memory displacement costs"
    );
    assert!(
        SCRIPT.contains("DIPECS_HIT_RATE_PCT") && SCRIPT.contains("STRONG_HIT_RATE_PCT"),
        "KeepAlive net-benefit collection must support same-budget DiPECS vs strong baseline inputs"
    );
    assert!(
        !SCRIPT.contains("\"accepted\": True"),
        "KeepAlive artifact acceptance must not be hard-coded"
    );
    assert!(
        SCRIPT.contains("\"accepted\": accepted"),
        "KeepAlive artifact acceptance must be derived from measured gates"
    );
}
