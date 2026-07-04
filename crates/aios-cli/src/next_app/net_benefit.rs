//! Net-benefit arithmetic for next-app PreWarm predictions.
//!
//! All inputs are measured or assumed externally; this module only provides the
//! deterministic cost/benefit combination used in evaluation reports.

use serde::{Deserialize, Serialize};

/// Inputs required to compute the net benefit of a ranker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetBenefitInputs {
    /// Top-1 hit rate expressed as a percentage (0.0-100.0).
    pub hit_rate_at_1_pct: f32,
    /// Measured startup time saved on a correct PreWarm (ms).
    pub prewarm_saved_ms: f64,
    /// Measured cost of a PreWarm that does not match the next app (ms).
    pub prewarm_wasted_ms: f64,
    /// DiPECS control-plane overhead per prediction (ms).
    pub control_plane_ms: f64,
}

/// A measured scalar input loaded from an evaluation fixture.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MeasuredValue {
    pub mean_ms: f64,
    #[serde(default)]
    pub p95_ms: Option<f64>,
    pub samples: usize,
    pub source: MeasurementSource,
}

impl MeasuredValue {
    pub fn validate(&self, name: &str) -> Result<(), String> {
        if !self.mean_ms.is_finite() || self.mean_ms < 0.0 {
            return Err(format!(
                "{name}.mean_ms must be a finite non-negative value"
            ));
        }
        if let Some(p95_ms) = self.p95_ms {
            if !p95_ms.is_finite() || p95_ms < 0.0 {
                return Err(format!("{name}.p95_ms must be a finite non-negative value"));
            }
        }
        if self.samples == 0 {
            return Err(format!("{name}.samples must be greater than zero"));
        }
        self.source.validate(name)
    }
}

/// Provenance for a measured value. A value may be conservative or derived from
/// measured counters, but it must not be an unlabelled placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MeasurementSource {
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl MeasurementSource {
    pub fn validate(&self, name: &str) -> Result<(), String> {
        if self.kind.trim().is_empty() {
            return Err(format!("{name}.source.kind must be non-empty"));
        }
        if self.path.trim().is_empty() {
            return Err(format!("{name}.source.path must be non-empty"));
        }
        let kind = self.kind.to_ascii_lowercase();
        if kind.contains("placeholder") || kind.contains("synthetic_constant") {
            return Err(format!("{name}.source.kind must not be a placeholder"));
        }
        Ok(())
    }
}

/// Fully measured PreWarmProcess net-benefit fixture.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PrewarmNetBenefitFixture {
    pub schema_version: String,
    pub dataset_id: String,
    pub action: String,
    pub status: String,
    pub trace: NetBenefitTrace,
    pub measurements: PrewarmMeasurements,
}

impl PrewarmNetBenefitFixture {
    pub const SCHEMA_VERSION: &'static str = "dipecs.action_net_benefit.prewarm.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(format!(
                "schema_version must be {}, got {}",
                Self::SCHEMA_VERSION,
                self.schema_version
            ));
        }
        if self.action != "PreWarmProcess" {
            return Err(format!(
                "action must be PreWarmProcess, got {}",
                self.action
            ));
        }
        if self.dataset_id.trim().is_empty() {
            return Err("dataset_id must be non-empty".into());
        }
        if self.status.to_ascii_lowercase().contains("placeholder") {
            return Err("status must not mark the fixture as placeholder".into());
        }
        if self.trace.source.trim().is_empty() {
            return Err("trace.source must be non-empty".into());
        }
        if self.trace.split.trim().is_empty() {
            return Err("trace.split must be non-empty".into());
        }
        if self.trace.examples == 0 {
            return Err("trace.examples must be greater than zero".into());
        }
        self.measurements
            .prewarm_saved
            .validate("measurements.prewarm_saved")?;
        self.measurements
            .wasted_prewarm
            .validate("measurements.wasted_prewarm")?;
        self.measurements
            .control_plane
            .dipecs
            .validate("measurements.control_plane.dipecs")?;
        self.measurements
            .control_plane
            .strong_baseline
            .validate("measurements.control_plane.strong_baseline")?;
        Ok(())
    }

    pub fn dipecs_inputs(&self, hit_rate_at_1_pct: f32) -> MeasuredNetBenefitInputs {
        MeasuredNetBenefitInputs {
            hit_rate_at_1_pct,
            prewarm_saved_ms: self.measurements.prewarm_saved.clone(),
            prewarm_wasted_ms: self.measurements.wasted_prewarm.clone(),
            control_plane_ms: self.measurements.control_plane.dipecs.clone(),
        }
    }

    pub fn strong_baseline_inputs(&self, hit_rate_at_1_pct: f32) -> MeasuredNetBenefitInputs {
        MeasuredNetBenefitInputs {
            hit_rate_at_1_pct,
            prewarm_saved_ms: self.measurements.prewarm_saved.clone(),
            prewarm_wasted_ms: self.measurements.wasted_prewarm.clone(),
            control_plane_ms: self.measurements.control_plane.strong_baseline.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NetBenefitTrace {
    pub source: String,
    pub split: String,
    pub examples: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PrewarmMeasurements {
    pub prewarm_saved: MeasuredValue,
    pub wasted_prewarm: MeasuredValue,
    pub control_plane: ControlPlaneMeasurements,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ControlPlaneMeasurements {
    pub dipecs: MeasuredValue,
    pub strong_baseline: MeasuredValue,
}

/// Inputs required to compute measured net benefit of a ranker.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredNetBenefitInputs {
    pub hit_rate_at_1_pct: f32,
    pub prewarm_saved_ms: MeasuredValue,
    pub prewarm_wasted_ms: MeasuredValue,
    pub control_plane_ms: MeasuredValue,
}

/// Inputs used to build a PreWarmProcess fixture from already-collected
/// offline measurements.
#[derive(Debug, Clone, PartialEq)]
pub struct PrewarmFixtureBuildInputs {
    pub dataset_id: String,
    pub status: String,
    pub trace: NetBenefitTrace,
    pub prewarm_saved: MeasuredValue,
    pub wasted_prewarm: MeasuredValue,
    pub dipecs_control_plane: MeasuredValue,
    pub strong_control_plane: MeasuredValue,
}

pub fn build_prewarm_net_benefit_fixture(
    inputs: PrewarmFixtureBuildInputs,
) -> Result<PrewarmNetBenefitFixture, String> {
    let fixture = PrewarmNetBenefitFixture {
        schema_version: PrewarmNetBenefitFixture::SCHEMA_VERSION.into(),
        dataset_id: inputs.dataset_id,
        action: "PreWarmProcess".into(),
        status: inputs.status,
        trace: inputs.trace,
        measurements: PrewarmMeasurements {
            prewarm_saved: inputs.prewarm_saved,
            wasted_prewarm: inputs.wasted_prewarm,
            control_plane: ControlPlaneMeasurements {
                dipecs: inputs.dipecs_control_plane,
                strong_baseline: inputs.strong_control_plane,
            },
        },
    };
    fixture.validate()?;
    Ok(fixture)
}

impl From<&MeasuredNetBenefitInputs> for NetBenefitInputs {
    fn from(inputs: &MeasuredNetBenefitInputs) -> Self {
        Self {
            hit_rate_at_1_pct: inputs.hit_rate_at_1_pct,
            prewarm_saved_ms: inputs.prewarm_saved_ms.mean_ms,
            prewarm_wasted_ms: inputs.prewarm_wasted_ms.mean_ms,
            control_plane_ms: inputs.control_plane_ms.mean_ms,
        }
    }
}

impl NetBenefitInputs {
    /// Build inputs whose `prewarm_wasted_ms` and `control_plane_ms` are
    /// **placeholders, not measurements**. Only `hit_rate_at_1_pct` and
    /// `prewarm_saved_ms` come from real data (the LSApp report and the
    /// `am start -W` UX fixture).
    ///
    /// TODO(#90): replace the placeholder wasted-prewarm and control-plane
    /// costs with real on-device measurements. Until then, any `net_benefit_ms`
    /// derived from these inputs is NOT a measured system benefit and must not
    /// be asserted on or cited as one; only `gross_saved_ms`, which depends
    /// solely on the two measured fields, is safe to gate on.
    pub fn placeholder_pending_measurement(hit_rate_at_1_pct: f32, prewarm_saved_ms: f64) -> Self {
        Self {
            hit_rate_at_1_pct,
            prewarm_saved_ms,
            // Placeholder; see TODO(#90) above. Not measured.
            prewarm_wasted_ms: 12.0,
            // Placeholder; see TODO(#90) above. Not measured.
            control_plane_ms: 0.0,
        }
    }
}

/// Result of a net-benefit computation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetBenefitReport {
    pub net_benefit_ms: f64,
    pub gross_saved_ms: f64,
    pub gross_wasted_ms: f64,
    pub control_plane_cost_ms: f64,
}

/// Compute net benefit for `examples` predictions.
///
/// - `gross_saved_ms = examples * hit_rate * saved_ms / 100.0`
/// - `gross_wasted_ms = examples * (1 - hit_rate/100.0) * wasted_ms`
/// - `control_plane_cost_ms = examples * control_plane_ms`
/// - `net_benefit_ms = gross_saved - gross_wasted - control_plane_cost`
pub fn compute_net_benefit(inputs: &NetBenefitInputs, examples: usize) -> NetBenefitReport {
    let examples_f = examples as f64;
    let hit = inputs.hit_rate_at_1_pct as f64;

    let gross_saved_ms = examples_f * hit * inputs.prewarm_saved_ms / 100.0;
    let gross_wasted_ms = examples_f * (1.0 - hit / 100.0) * inputs.prewarm_wasted_ms;
    let control_plane_cost_ms = examples_f * inputs.control_plane_ms;
    let net_benefit_ms = gross_saved_ms - gross_wasted_ms - control_plane_cost_ms;

    NetBenefitReport {
        net_benefit_ms,
        gross_saved_ms,
        gross_wasted_ms,
        control_plane_cost_ms,
    }
}

/// Compute net benefit from fixture-backed measured inputs.
pub fn compute_measured_net_benefit(
    inputs: &MeasuredNetBenefitInputs,
    examples: usize,
) -> Result<NetBenefitReport, String> {
    inputs.prewarm_saved_ms.validate("prewarm_saved_ms")?;
    inputs.prewarm_wasted_ms.validate("prewarm_wasted_ms")?;
    inputs.control_plane_ms.validate("control_plane_ms")?;
    Ok(compute_net_benefit(
        &NetBenefitInputs::from(inputs),
        examples,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> MeasurementSource {
        MeasurementSource {
            kind: "measured_fixture".into(),
            path: "data/evaluation/example.json".into(),
            field: None,
            note: None,
        }
    }

    fn value(mean_ms: f64) -> MeasuredValue {
        MeasuredValue {
            mean_ms,
            p95_ms: None,
            samples: 10,
            source: source(),
        }
    }

    fn inputs(hit: f32, saved: f64, wasted: f64, control: f64) -> NetBenefitInputs {
        NetBenefitInputs {
            hit_rate_at_1_pct: hit,
            prewarm_saved_ms: saved,
            prewarm_wasted_ms: wasted,
            control_plane_ms: control,
        }
    }

    #[test]
    fn zero_examples_yields_zero_benefit() {
        let report = compute_net_benefit(&inputs(50.0, 100.0, 10.0, 1.0), 0);
        assert_eq!(report.net_benefit_ms, 0.0);
        assert_eq!(report.gross_saved_ms, 0.0);
        assert_eq!(report.gross_wasted_ms, 0.0);
        assert_eq!(report.control_plane_cost_ms, 0.0);
    }

    #[test]
    fn perfect_hit_rate_avoids_waste() {
        let report = compute_net_benefit(&inputs(100.0, 80.0, 20.0, 0.5), 10);
        assert!((report.gross_saved_ms - 800.0).abs() < 1e-9);
        assert!((report.gross_wasted_ms - 0.0).abs() < 1e-9);
        assert!((report.control_plane_cost_ms - 5.0).abs() < 1e-9);
        assert!((report.net_benefit_ms - 795.0).abs() < 1e-9);
    }

    #[test]
    fn zero_hit_rate_is_pure_cost() {
        let report = compute_net_benefit(&inputs(0.0, 80.0, 15.0, 0.5), 20);
        assert!((report.gross_saved_ms - 0.0).abs() < 1e-9);
        assert!((report.gross_wasted_ms - 300.0).abs() < 1e-9);
        assert!((report.control_plane_cost_ms - 10.0).abs() < 1e-9);
        assert!((report.net_benefit_ms - (-310.0)).abs() < 1e-9);
    }

    #[test]
    fn mixed_hit_rate_matches_formula() {
        // 100 examples, 40% hit, saved=200ms, wasted=20ms, control=2ms.
        // saved = 100 * 0.4 * 200 = 8000
        // wasted = 100 * 0.6 * 20 = 1200
        // control = 100 * 2 = 200
        // net = 6600
        let report = compute_net_benefit(&inputs(40.0, 200.0, 20.0, 2.0), 100);
        assert!((report.gross_saved_ms - 8000.0).abs() < 1e-9);
        assert!((report.gross_wasted_ms - 1200.0).abs() < 1e-9);
        assert!((report.control_plane_cost_ms - 200.0).abs() < 1e-9);
        assert!((report.net_benefit_ms - 6600.0).abs() < 1e-9);
    }

    #[test]
    fn measured_inputs_reject_placeholder_sources() {
        let mut measured = MeasuredNetBenefitInputs {
            hit_rate_at_1_pct: 50.0,
            prewarm_saved_ms: value(100.0),
            prewarm_wasted_ms: value(10.0),
            control_plane_ms: value(1.0),
        };
        measured.control_plane_ms.source.kind = "placeholder".into();

        let err = compute_measured_net_benefit(&measured, 10)
            .expect_err("placeholder measured input must be rejected");
        assert!(err.contains("placeholder"));
    }

    fn valid_build_inputs() -> PrewarmFixtureBuildInputs {
        PrewarmFixtureBuildInputs {
            dataset_id: "fixture-test".into(),
            status: "measured".into(),
            trace: NetBenefitTrace {
                source: "data/evaluation/next-app/lsapp-standard.report.json".into(),
                split: "Standard".into(),
                examples: 100,
            },
            prewarm_saved: value(100.0),
            wasted_prewarm: value(10.0),
            dipecs_control_plane: value(1.0),
            strong_control_plane: value(0.5),
        }
    }

    #[test]
    fn prewarm_fixture_builder_rejects_bad_action_shape() {
        let mut fixture =
            build_prewarm_net_benefit_fixture(valid_build_inputs()).expect("fixture should build");
        fixture.action = "ReleaseMemory".into();

        let err = fixture
            .validate()
            .expect_err("wrong action type must be rejected");
        assert!(err.contains("PreWarmProcess"));
    }

    #[test]
    fn prewarm_fixture_builder_rejects_zero_examples() {
        let mut inputs = valid_build_inputs();
        inputs.trace.examples = 0;

        let err =
            build_prewarm_net_benefit_fixture(inputs).expect_err("zero examples must be rejected");
        assert!(err.contains("trace.examples"));
    }

    #[test]
    fn prewarm_fixture_builder_rejects_zero_samples() {
        let mut inputs = valid_build_inputs();
        inputs.wasted_prewarm.samples = 0;

        let err =
            build_prewarm_net_benefit_fixture(inputs).expect_err("zero samples must be rejected");
        assert!(err.contains("samples"));
    }

    #[test]
    fn prewarm_fixture_builder_rejects_negative_measurements() {
        let mut inputs = valid_build_inputs();
        inputs.dipecs_control_plane.mean_ms = -0.1;

        let err = build_prewarm_net_benefit_fixture(inputs)
            .expect_err("negative measurements must be rejected");
        assert!(err.contains("mean_ms"));
    }

    #[test]
    fn prewarm_fixture_builder_rejects_placeholder_status() {
        let mut inputs = valid_build_inputs();
        inputs.status = "placeholder_pending_measurement".into();

        let err = build_prewarm_net_benefit_fixture(inputs)
            .expect_err("placeholder status must be rejected");
        assert!(err.contains("placeholder"));
    }

    #[test]
    fn measured_value_validation_rejects_many_bad_shapes() {
        let cases = [
            (
                "negative_mean",
                -1.0,
                Some(1.0),
                10,
                "measured_fixture",
                "x",
            ),
            ("negative_p95", 1.0, Some(-1.0), 10, "measured_fixture", "x"),
            ("zero_samples", 1.0, Some(1.0), 0, "measured_fixture", "x"),
            ("empty_kind", 1.0, Some(1.0), 10, "", "x"),
            ("empty_path", 1.0, Some(1.0), 10, "measured_fixture", ""),
            (
                "placeholder_kind",
                1.0,
                Some(1.0),
                10,
                "placeholder_cost",
                "x",
            ),
            (
                "synthetic_constant_kind",
                1.0,
                Some(1.0),
                10,
                "synthetic_constant",
                "x",
            ),
        ];

        for (name, mean_ms, p95_ms, samples, kind, path) in cases {
            let bad = MeasuredValue {
                mean_ms,
                p95_ms,
                samples,
                source: MeasurementSource {
                    kind: kind.into(),
                    path: path.into(),
                    field: None,
                    note: None,
                },
            };
            assert!(
                bad.validate(name).is_err(),
                "{name} should fail measured value validation"
            );
        }
    }

    #[test]
    fn prewarm_fixture_validation_rejects_many_corrupt_fixture_variants() {
        type Mutator = fn(&mut PrewarmFixtureBuildInputs);
        let cases: [(&str, Mutator); 16] = [
            ("empty_dataset", |i| i.dataset_id.clear()),
            ("placeholder_status", |i| i.status = "placeholder".into()),
            ("empty_trace_source", |i| i.trace.source.clear()),
            ("empty_trace_split", |i| i.trace.split.clear()),
            ("zero_examples", |i| i.trace.examples = 0),
            ("negative_saved", |i| i.prewarm_saved.mean_ms = -0.01),
            ("negative_saved_p95", |i| {
                i.prewarm_saved.p95_ms = Some(-0.01)
            }),
            ("zero_saved_samples", |i| i.prewarm_saved.samples = 0),
            ("negative_wasted", |i| i.wasted_prewarm.mean_ms = -0.01),
            ("zero_wasted_samples", |i| i.wasted_prewarm.samples = 0),
            ("negative_dipecs_control", |i| {
                i.dipecs_control_plane.mean_ms = -0.01
            }),
            ("zero_dipecs_control_samples", |i| {
                i.dipecs_control_plane.samples = 0
            }),
            ("negative_strong_control", |i| {
                i.strong_control_plane.mean_ms = -0.01
            }),
            ("zero_strong_control_samples", |i| {
                i.strong_control_plane.samples = 0
            }),
            ("placeholder_source", |i| {
                i.prewarm_saved.source.kind = "placeholder".into()
            }),
            ("empty_source_path", |i| i.prewarm_saved.source.path.clear()),
        ];

        for (name, mutate) in cases {
            let mut inputs = valid_build_inputs();
            mutate(&mut inputs);
            assert!(
                build_prewarm_net_benefit_fixture(inputs).is_err(),
                "{name} should fail fixture validation"
            );
        }
    }

    #[test]
    fn net_benefit_formula_matches_large_deterministic_case_matrix() {
        let hit_rates = [0.0_f32, 1.0, 25.0, 50.0, 75.0, 99.0, 100.0];
        let saved_values = [0.0_f64, 1.0, 31.231, 394.8, 1_000.0];
        let wasted_values = [0.0_f64, 1.0, 12.0, 31.231, 250.0];
        let control_values = [0.0_f64, 0.07848, 1.0, 10.0];
        let example_counts = [0_usize, 1, 7, 100, 272_519];

        let mut checked = 0usize;
        for hit in hit_rates {
            for saved in saved_values {
                for wasted in wasted_values {
                    for control in control_values {
                        for examples in example_counts {
                            let inputs = NetBenefitInputs {
                                hit_rate_at_1_pct: hit,
                                prewarm_saved_ms: saved,
                                prewarm_wasted_ms: wasted,
                                control_plane_ms: control,
                            };
                            let report = compute_net_benefit(&inputs, examples);
                            let examples_f = examples as f64;
                            let hit_f = hit as f64 / 100.0;
                            let expected_saved = examples_f * hit_f * saved;
                            let expected_wasted = examples_f * (1.0 - hit_f) * wasted;
                            let expected_control = examples_f * control;
                            let expected_net = expected_saved - expected_wasted - expected_control;

                            assert!((report.gross_saved_ms - expected_saved).abs() < 1e-6);
                            assert!((report.gross_wasted_ms - expected_wasted).abs() < 1e-6);
                            assert!((report.control_plane_cost_ms - expected_control).abs() < 1e-6);
                            assert!((report.net_benefit_ms - expected_net).abs() < 1e-6);
                            checked += 1;
                        }
                    }
                }
            }
        }

        assert_eq!(checked, 7 * 5 * 5 * 4 * 5);
    }

    #[test]
    fn measured_net_benefit_monotonicity_holds_over_many_cases() {
        let examples = 1_000;
        let source = source();
        let mut checked = 0usize;

        for hit in [0.0_f32, 10.0, 50.0, 90.0, 100.0] {
            for saved in [1.0_f64, 100.0, 394.8] {
                for wasted in [0.0_f64, 10.0, 31.231] {
                    for control in [0.0_f64, 0.07848, 5.0] {
                        let base = MeasuredNetBenefitInputs {
                            hit_rate_at_1_pct: hit,
                            prewarm_saved_ms: MeasuredValue {
                                mean_ms: saved,
                                p95_ms: None,
                                samples: 10,
                                source: source.clone(),
                            },
                            prewarm_wasted_ms: MeasuredValue {
                                mean_ms: wasted,
                                p95_ms: None,
                                samples: 10,
                                source: source.clone(),
                            },
                            control_plane_ms: MeasuredValue {
                                mean_ms: control,
                                p95_ms: None,
                                samples: 10,
                                source: source.clone(),
                            },
                        };
                        let base_report =
                            compute_measured_net_benefit(&base, examples).expect("valid base");

                        let mut more_saved = base.clone();
                        more_saved.prewarm_saved_ms.mean_ms += 1.0;
                        let more_saved_report =
                            compute_measured_net_benefit(&more_saved, examples).unwrap();
                        assert!(more_saved_report.net_benefit_ms >= base_report.net_benefit_ms);

                        let mut more_waste = base.clone();
                        more_waste.prewarm_wasted_ms.mean_ms += 1.0;
                        let more_waste_report =
                            compute_measured_net_benefit(&more_waste, examples).unwrap();
                        assert!(more_waste_report.net_benefit_ms <= base_report.net_benefit_ms);

                        let mut more_control = base.clone();
                        more_control.control_plane_ms.mean_ms += 1.0;
                        let more_control_report =
                            compute_measured_net_benefit(&more_control, examples).unwrap();
                        assert!(more_control_report.net_benefit_ms <= base_report.net_benefit_ms);
                        checked += 1;
                    }
                }
            }
        }

        assert_eq!(checked, 5 * 3 * 3 * 3);
    }

    #[test]
    fn measured_net_benefit_matches_raw_formula_over_large_fixture_like_grid() {
        let source = source();
        let hit_rates = [
            0.0_f32, 0.1, 1.0, 5.0, 12.5, 25.0, 33.333, 50.0, 56.442, 75.0, 90.0, 99.9, 100.0,
        ];
        let saved_values = [0.001_f64, 1.0, 12.0, 31.231, 120.0, 394.8, 805.8, 1_500.0];
        let wasted_values = [0.0_f64, 0.001, 1.0, 12.0, 31.231, 120.0, 394.8];
        let control_values = [0.0_f64, 0.0001, 0.07848, 0.5, 1.0, 7.5, 31.231];
        let example_counts = [1_usize, 2, 17, 999, 10_000, 272_519, 1_000_000];

        let mut checked = 0usize;
        for hit_rate_at_1_pct in hit_rates {
            for saved in saved_values {
                for wasted in wasted_values {
                    for control in control_values {
                        for examples in example_counts {
                            let inputs = MeasuredNetBenefitInputs {
                                hit_rate_at_1_pct,
                                prewarm_saved_ms: MeasuredValue {
                                    mean_ms: saved,
                                    p95_ms: Some(saved),
                                    samples: 30,
                                    source: source.clone(),
                                },
                                prewarm_wasted_ms: MeasuredValue {
                                    mean_ms: wasted,
                                    p95_ms: Some(wasted),
                                    samples: 30,
                                    source: source.clone(),
                                },
                                control_plane_ms: MeasuredValue {
                                    mean_ms: control,
                                    p95_ms: Some(control),
                                    samples: 30,
                                    source: source.clone(),
                                },
                            };
                            let report = compute_measured_net_benefit(&inputs, examples).unwrap();
                            let examples_f = examples as f64;
                            let hit = hit_rate_at_1_pct as f64 / 100.0;
                            let expected = examples_f * hit * saved
                                - examples_f * (1.0 - hit) * wasted
                                - examples_f * control;
                            assert!(
                                (report.net_benefit_ms - expected).abs() < 1e-5,
                                "case hit={hit_rate_at_1_pct} saved={saved} wasted={wasted} control={control} examples={examples}"
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }

        assert_eq!(checked, 13 * 8 * 7 * 7 * 7);
    }

    #[test]
    fn break_even_threshold_cases_stay_numerically_stable() {
        let examples = 100_000;
        let saved_values = [1.0_f64, 31.231, 394.8, 805.8, 1_500.0];
        let wasted_values = [0.0_f64, 1.0, 12.0, 31.231, 394.8];
        let control_values = [0.0_f64, 0.07848, 1.0, 12.0, 31.231];
        let mut checked = 0usize;

        for saved in saved_values {
            for wasted in wasted_values {
                for control in control_values {
                    let denominator = saved + wasted;
                    if denominator <= 0.0 {
                        continue;
                    }
                    let break_even_hit =
                        ((wasted + control) / denominator * 100.0).clamp(0.0, 100.0) as f32;
                    for offset in [-0.01_f32, 0.0, 0.01] {
                        let hit = (break_even_hit + offset).clamp(0.0, 100.0);
                        let report = compute_net_benefit(
                            &NetBenefitInputs {
                                hit_rate_at_1_pct: hit,
                                prewarm_saved_ms: saved,
                                prewarm_wasted_ms: wasted,
                                control_plane_ms: control,
                            },
                            examples,
                        );
                        assert!(report.net_benefit_ms.is_finite());
                        if offset < 0.0 && break_even_hit > 0.01 && break_even_hit < 99.99 {
                            assert!(
                                report.net_benefit_ms <= 1e-3,
                                "below break-even should not be positive: saved={saved} wasted={wasted} control={control} hit={hit}"
                            );
                        }
                        if offset > 0.0 && break_even_hit > 0.01 && break_even_hit < 99.99 {
                            assert!(
                                report.net_benefit_ms >= -1e-3,
                                "above break-even should not be negative: saved={saved} wasted={wasted} control={control} hit={hit}"
                            );
                        }
                        checked += 1;
                    }
                }
            }
        }

        assert_eq!(checked, 5 * 5 * 5 * 3);
    }

    #[test]
    fn fixture_builder_accepts_many_valid_fixture_shapes() {
        let source = source();
        let mut checked = 0usize;

        for examples in [1_usize, 10, 1_000, 272_519, 1_000_000] {
            for saved in [0.001_f64, 31.231, 394.8, 1_500.0] {
                for wasted in [0.0_f64, 0.001, 31.231, 394.8] {
                    for control in [0.0_f64, 0.0001, 0.07848, 10.0] {
                        let fixture =
                            build_prewarm_net_benefit_fixture(PrewarmFixtureBuildInputs {
                                dataset_id: format!(
                                    "fixture-{examples}-{saved}-{wasted}-{control}"
                                ),
                                status: "generated_from_offline_measurements".into(),
                                trace: NetBenefitTrace {
                                    source: "data/evaluation/next-app/lsapp-standard.report.json"
                                        .into(),
                                    split: "Standard".into(),
                                    examples,
                                },
                                prewarm_saved: MeasuredValue {
                                    mean_ms: saved,
                                    p95_ms: Some(saved),
                                    samples: 20,
                                    source: source.clone(),
                                },
                                wasted_prewarm: MeasuredValue {
                                    mean_ms: wasted,
                                    p95_ms: Some(wasted),
                                    samples: 1,
                                    source: source.clone(),
                                },
                                dipecs_control_plane: MeasuredValue {
                                    mean_ms: control,
                                    p95_ms: None,
                                    samples: 1_631,
                                    source: source.clone(),
                                },
                                strong_control_plane: MeasuredValue {
                                    mean_ms: 0.0,
                                    p95_ms: None,
                                    samples: examples,
                                    source: source.clone(),
                                },
                            })
                            .expect("valid fixture shape should build");
                        fixture.validate().expect("valid fixture should validate");
                        checked += 1;
                    }
                }
            }
        }

        assert_eq!(checked, 5 * 4 * 4 * 4);
    }
}
