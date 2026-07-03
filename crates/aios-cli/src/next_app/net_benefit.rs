//! Net-benefit arithmetic for next-app PreWarm predictions.
//!
//! All inputs are measured or assumed externally; this module only provides the
//! deterministic cost/benefit combination used in evaluation reports.

/// Inputs required to compute the net benefit of a ranker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetBenefitInputs {
    /// Top-1 hit rate expressed as a percentage (0.0–100.0).
    pub hit_rate_at_1_pct: f32,
    /// Measured startup time saved on a correct PreWarm (ms).
    pub prewarm_saved_ms: f64,
    /// Measured cost of a PreWarm that does not match the next app (ms).
    pub prewarm_wasted_ms: f64,
    /// DiPECS control-plane overhead per prediction (ms).
    pub control_plane_ms: f64,
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
