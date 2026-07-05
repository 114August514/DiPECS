use super::support::{find_keepalive_memory_pressure_report, load_json};

const SCRIPT: &str = include_str!("../../../../tools/collect/collect-keepalive-memory-pressure.sh");

#[test]
fn keepalive_collection_script_has_pressure_and_safety_gates() {
    assert!(SCRIPT.contains("emulator-dry-run"));
    assert!(SCRIPT.contains("real-device-calibrate"));
    assert!(SCRIPT.contains("real-device-collect"));
    assert!(SCRIPT.contains("pressure_insufficient"));
    assert!(SCRIPT.contains("safety_stopped"));
    assert!(SCRIPT.contains("temperature_stop_c"));
    assert!(SCRIPT.contains("DebugMemoryPressureService"));
    assert!(SCRIPT.contains("KeepAlive"));
    assert!(SCRIPT.contains("work:collector_heartbeat"));
    assert!(SCRIPT.contains("status=ok"));
    assert!(!SCRIPT.contains("\"accepted\": true"));
    assert!(SCRIPT.contains("\"accepted\": accepted"));
    // Integrity guards (reviewer C1/C2/I1): acceptance must require the KeepAlive
    // system mechanism to have actually engaged, the OOM counter must exclude the
    // harness's own pressure processes, and the baseline-hit / benefit thresholds
    // must require a minimum absolute event count, not a single flaky sample.
    assert!(
        SCRIPT.contains("mechanism_engaged"),
        "accept gate must verify the KeepAlive mechanism engaged (oom+cgroup), \
         else a noise delta is miscredited to an inert KeepAlive"
    );
    assert!(
        SCRIPT.contains("and mechanism_engaged"),
        "mechanism_engaged must be a hard precondition of accepted"
    );
    assert!(
        SCRIPT.contains(":pressure"),
        "oom_event_count must exclude the harness's own :pressure processes"
    );
    assert!(
        SCRIPT.contains("no_kill_or_restart >= 3"),
        "native_baseline_hit must require a minimum absolute kill/restart count"
    );
}

#[test]
fn keepalive_memory_pressure_fixture_schema_and_gates() {
    // LOUD SKIP (not a silent pass): a KeepAlive benefit is only measurable under
    // a platform-signed system deployment where dipecsd (a native, non-app process
    // not managed by AMS) can lower oom_score_adj and have it PERSIST. On a normal
    // app the mechanism is inert (oom/cgroup writes denied -> JobScheduler fallback)
    // and even a root-written oom_score_adj is clobbered by AMS on app state change.
    // So no accepted n>=20 real-device fixture exists yet. When one is produced by a
    // system deployment (see follow-up issue), commit it as the -fixture.json and
    // this test validates its schema + integrity gates. Until then, skip loudly.
    let Some(path) = find_keepalive_memory_pressure_report() else {
        eprintln!(
            "\n############################################################\n\
             # SKIPPED keepalive_memory_pressure_fixture: no fixture.   #\n\
             # KeepAlive's system mechanism (oom_score_adj/cgroup) is   #\n\
             # inert on a normal app and AMS-clobbered even when root-  #\n\
             # written; a real benefit needs platform-signed dipecsd.   #\n\
             # This is a documented conditional skip, not a pass.       #\n\
             ############################################################\n"
        );
        return;
    };
    let report = load_json(&path).expect("KeepAlive memory-pressure fixture must parse");

    assert_eq!(
        report.get("schema_version").and_then(|v| v.as_str()),
        Some("dipecs.keepalive_memory_pressure.v1"),
        "{} must use the KeepAlive memory-pressure schema",
        path.display()
    );
    assert_eq!(
        report.get("issue").and_then(|v| v.as_u64()),
        Some(98),
        "{} must be tied to issue #98",
        path.display()
    );
    assert_eq!(
        report.get("source").and_then(|v| v.as_str()),
        Some("measured_device"),
        "{} must be measured_device, not emulator-only evidence",
        path.display()
    );
    assert_eq!(
        report.get("status").and_then(|v| v.as_str()),
        Some("measured_android_real_device"),
        "{} must be a real-device measurement",
        path.display()
    );

    let safety = report.get("safety").expect("safety block must be present");
    assert_eq!(
        safety.get("safety_stopped").and_then(|v| v.as_bool()),
        Some(false),
        "{} must not be a safety-stopped run",
        path.display()
    );
    assert!(
        safety
            .get("temperature_stop_c")
            .and_then(|v| v.as_f64())
            .unwrap_or_default()
            <= 42.0,
        "{} must use a conservative temperature stop",
        path.display()
    );

    let calibration = report
        .get("calibration")
        .expect("calibration block must be present");
    assert_eq!(
        calibration.get("pressure_valid").and_then(|v| v.as_bool()),
        Some(true),
        "{} must prove native Android baseline was stressed before closing #98",
        path.display()
    );

    let runs = report
        .get("runs")
        .and_then(|v| v.as_array())
        .expect("runs must be present");
    for expected_mode in ["no_keepalive_pressure", "keepalive_pressure"] {
        let run = runs
            .iter()
            .find(|run| run.get("mode").and_then(|v| v.as_str()) == Some(expected_mode))
            .unwrap_or_else(|| panic!("{expected_mode} missing from {}", path.display()));
        let samples = run
            .get("samples")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("{expected_mode} samples missing from {}", path.display()));
        assert!(
            samples.len() >= 20,
            "{expected_mode} must have n>=20 samples in {}",
            path.display()
        );
        let summary = run.get("summary").expect("summary must be present");
        assert_eq!(
            summary.get("n").and_then(|v| v.as_u64()),
            Some(samples.len() as u64),
            "{expected_mode} summary.n must match samples"
        );
        for field in [
            "survival_rate_pct",
            "restart_rate_pct",
            "p95_return_total_time_ms",
            "mean_jank_pct",
            "mean_available_after_mb",
            "oom_event_count",
        ] {
            assert!(
                summary.get(field).and_then(|v| v.as_f64()).is_some()
                    || summary.get(field).and_then(|v| v.as_i64()).is_some()
                    || summary.get(field).and_then(|v| v.as_u64()).is_some(),
                "{expected_mode} summary.{field} missing from {}",
                path.display()
            );
        }
        for sample in samples {
            for field in [
                "target_pid_before",
                "target_pid_after",
                "target_survived",
                "target_restarted",
                "return_total_time_ms",
                "pressure_available_min_mb",
                "available_before_mb",
                "available_after_mb",
                "memtotal_mb",
                "target_pss_before_mb",
                "target_pss_after_mb",
                "jank_pct",
                "oom_event_count",
            ] {
                assert!(
                    sample.get(field).is_some(),
                    "{expected_mode} sample missing {field} in {}",
                    path.display()
                );
            }
            if expected_mode == "keepalive_pressure" {
                assert!(
                    sample.get("keepalive_device_status").is_some(),
                    "keepalive_pressure sample must record bridge status"
                );
                assert!(
                    sample
                        .get("oom_score_adjusted")
                        .and_then(|v| v.as_bool())
                        .is_some(),
                    "keepalive_pressure sample must record oom_score_adjusted"
                );
                assert!(
                    sample
                        .get("cgroup_pinned")
                        .and_then(|v| v.as_bool())
                        .is_some(),
                    "keepalive_pressure sample must record cgroup_pinned"
                );
            }
        }
    }

    let comparison = report
        .get("strong_baseline_comparison")
        .expect("strong_baseline_comparison must be present");
    for model in ["dipecs_ensemble", "strong_predictive"] {
        let model = comparison
            .get(model)
            .unwrap_or_else(|| panic!("{model} missing from strong_baseline_comparison"));
        assert!(
            model
                .get("hit_rate_at_1_pct")
                .and_then(|v| v.as_f64())
                .unwrap_or_default()
                > 0.0,
            "baseline model must carry hit@1"
        );
        assert!(
            model
                .get("action_budget")
                .and_then(|v| v.as_u64())
                .is_some(),
            "baseline model must carry action_budget"
        );
        assert!(
            model
                .get("net_benefit_score")
                .and_then(|v| v.as_f64())
                .is_some(),
            "baseline model must carry net_benefit_score"
        );
    }

    let summary = report.get("summary").expect("summary must be present");
    let conclusion = report
        .get("conclusion")
        .expect("conclusion must be present");
    // Mechanism engagement (reviewer C1): acceptance requires the KeepAlive system
    // action to have engaged (oom+cgroup) on every keepalive-arm sample. Recompute
    // it from the run summary so the guard mirrors the script's accept gate rather
    // than trusting a stored boolean.
    let keepalive_run = runs
        .iter()
        .find(|run| run.get("mode").and_then(|v| v.as_str()) == Some("keepalive_pressure"))
        .expect("keepalive_pressure run must be present");
    let keepalive_summary = keepalive_run
        .get("summary")
        .expect("keepalive_pressure summary must be present");
    let keepalive_n = keepalive_summary
        .get("n")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    // Mechanism gate is oom-primary (see script): oom_score_adj is load-bearing;
    // the cpuset pin is secondary/kernel-dependent, so acceptance keys on
    // oom_engaged_count, not the stricter oom+cgroup mechanism_engaged_count.
    let mechanism_engaged = keepalive_n > 0
        && keepalive_summary
            .get("oom_engaged_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            == keepalive_n;
    let recomputed_accepted = conclusion
        .get("pressure_valid")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        && conclusion
            .get("n_at_least_20_per_mode")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        && mechanism_engaged
        && conclusion
            .get("native_baseline_hit")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        && conclusion
            .get("directional_benefit")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        && conclusion
            .get("user_cost_absent")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        && conclusion
            .get("memory_cost_absent")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        && summary
            .get("net_benefit_score")
            .and_then(|v| v.as_f64())
            .unwrap_or_default()
            > 0.0;
    assert_eq!(
        conclusion.get("accepted").and_then(|v| v.as_bool()),
        Some(recomputed_accepted),
        "accepted must be derived from pressure, sample, benefit, and cost gates"
    );
}
