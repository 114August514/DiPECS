const SCRIPT: &str = include_str!("../../../../tools/collect/collect-prewarm-net-benefit.sh");

#[test]
fn collection_script_fails_closed_on_missing_measurements() {
    assert!(
        SCRIPT.contains("send_prewarm missing device response"),
        "PreWarm collection must fail when the bridge response cannot be parsed"
    );
    assert!(
        SCRIPT.contains("send_prewarm bridge did not accept action"),
        "PreWarm collection must require an ok bridge response"
    );
    assert!(
        SCRIPT.contains("startup TotalTime missing or non-positive"),
        "startup collection must fail when am start -W omits a positive TotalTime"
    );
    assert!(
        !SCRIPT.contains("\"accepted\": True"),
        "net-benefit artifact acceptance must not be hard-coded"
    );
    assert!(
        SCRIPT.contains("\"accepted\": accepted"),
        "net-benefit artifact acceptance must be derived from measured gates"
    );
}
