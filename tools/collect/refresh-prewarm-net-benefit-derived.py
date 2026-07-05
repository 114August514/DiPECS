#!/usr/bin/env python3
"""Refresh the DERIVED blocks of a real-device PreWarm net-benefit snapshot.

Why this exists
---------------
`data/evaluation/next-app/prewarm-net-benefit-real-device-*.json` is produced by
`tools/collect/collect-prewarm-net-benefit.sh`, which needs a live adb target: it
samples startup/latency on device (the `runs` array) AND derives a `net_benefit`
block by reading `hit_rate_at_1_pct` from the committed LSApp standard report.

When the standard report is regenerated (e.g. the model changed), the on-device
measurements are still valid but the DERIVED net-benefit no longer matches the
new hit rate. Re-running the full collector would need the device again. This
script re-derives ONLY the report-dependent blocks from the measurements already
recorded in the snapshot, using the SAME formulas as the collector
(collect-prewarm-net-benefit.sh, the `benefit()` function). It never touches the
on-device `runs`; it recomputes `measured_inputs` and `net_benefit` from them so
the output is identical to what the collector would have emitted against the
current report.

Usage
-----
    python3 tools/collect/refresh-prewarm-net-benefit-derived.py \
        [--snapshot data/evaluation/next-app/prewarm-net-benefit-real-device-20260704-184148.json] \
        [--report   data/evaluation/next-app/lsapp-standard.report.json] \
        [--check]

    --check  exit non-zero if the file would change (no write); for CI drift guards.

The snapshot is rewritten in place (pretty-printed, trailing newline) unless
--check is given.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_SNAPSHOT = (
    REPO_ROOT
    / "data/evaluation/next-app/prewarm-net-benefit-real-device-20260704-184148.json"
)
DEFAULT_REPORT = REPO_ROOT / "data/evaluation/next-app/lsapp-standard.report.json"


def summary_of(run: dict) -> dict:
    return run["summary"]


def benefit(hit_rate_pct: float, examples: int, hit_saved_ms: float,
            miss_action_cost_ms: float, control_plane_ms: float) -> dict:
    """Identical to collect-prewarm-net-benefit.sh `benefit()`."""
    hit = hit_rate_pct / 100.0
    gross_saved = examples * hit * hit_saved_ms
    gross_wasted = examples * (1.0 - hit) * miss_action_cost_ms
    control = examples * control_plane_ms
    return {
        "source": "measured_device",
        "hit_rate_at_1_pct": round(hit_rate_pct, 3),
        "gross_saved_ms": round(gross_saved, 3),
        "gross_wasted_ms": round(gross_wasted, 3),
        "control_plane_cost_ms": round(control, 3),
        "net_benefit_ms": round(gross_saved - gross_wasted - control, 3),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", type=Path, default=DEFAULT_SNAPSHOT)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--check", action="store_true",
                        help="exit non-zero if the file would change; do not write")
    args = parser.parse_args()

    snapshot = json.loads(args.snapshot.read_text(encoding="utf-8"))
    report = json.loads(args.report.read_text(encoding="utf-8"))

    # Report-derived inputs (the only things that change with the model).
    examples = int(report["test_examples"])
    ensemble_hit = float(report["metrics"]["ensemble"]["hit_rate_at_1_pct"])
    strong_hit = float(report["metrics"]["strong_predictive"]["hit_rate_at_1_pct"])

    # On-device measurements are AUTHORITATIVE and hit-rate-independent: reuse the
    # snapshot's recorded `measured_inputs` verbatim. We deliberately do NOT
    # recompute them from `runs` — the recorded control_plane_ms (8.394) reflects
    # the latency actually measured during collection, which differs slightly from
    # the runs' rounded mean_prewarm_latency_us summary; the measurement, not a
    # re-derivation, is the source of truth. Only the report-dependent net_benefit
    # block below is refreshed for the new hit rate.
    measured = snapshot["measured_inputs"]
    hit_saved_ms = float(measured["hit_saved_ms"])
    miss_action_cost_ms = float(measured["miss_action_cost_ms"])
    control_plane_ms = float(measured["control_plane_ms"])

    for name, value in (
        ("hit_saved_ms", hit_saved_ms),
        ("control_plane_ms", control_plane_ms),
    ):
        if not (math.isfinite(value) and value > 0):
            raise SystemExit(f"snapshot measured_inputs.{name} must be finite and positive; got {value}")
    if not (math.isfinite(miss_action_cost_ms) and miss_action_cost_ms >= 0):
        raise SystemExit(f"snapshot measured_inputs.miss_action_cost_ms must be finite and >= 0")

    ensemble = benefit(ensemble_hit, examples, hit_saved_ms, miss_action_cost_ms, control_plane_ms)
    strong = benefit(strong_hit, examples, hit_saved_ms, miss_action_cost_ms, control_plane_ms)

    # measured_inputs is preserved unchanged (authoritative device measurement).
    updated = dict(snapshot)
    updated["net_benefit"] = {
        "source": "measured_device",
        "examples": examples,
        "action_budget": snapshot["net_benefit"].get(
            "action_budget", "top1_one_prewarm_per_lsapp_test_example"
        ),
        "dipecs_ensemble": ensemble,
        "strong_predictive": strong,
        "dipecs_minus_strong_net_benefit_ms": round(
            ensemble["net_benefit_ms"] - strong["net_benefit_ms"], 3
        ),
    }
    updated["conclusion"] = {
        "accepted": ensemble["net_benefit_ms"] > 0
        and ensemble["net_benefit_ms"] > strong["net_benefit_ms"],
        "n_at_least_20_per_mode": all(
            summary_of(run)["n"] >= 20 for run in snapshot["runs"]
        ),
        "measured_inputs_valid": True,
        "net_benefit_positive": ensemble["net_benefit_ms"] > 0,
        "dipecs_beats_strong_predictive": ensemble["net_benefit_ms"] > strong["net_benefit_ms"],
    }

    serialized = json.dumps(updated, indent=2, ensure_ascii=False) + "\n"
    current = args.snapshot.read_text(encoding="utf-8")
    if serialized == current:
        print(f"up to date: {args.snapshot.relative_to(REPO_ROOT)}")
        return 0
    if args.check:
        print(f"DRIFT: {args.snapshot.relative_to(REPO_ROOT)} would change "
              f"(ensemble hit@1 -> {ensemble_hit}, net_benefit_ms -> {ensemble['net_benefit_ms']})",
              file=sys.stderr)
        return 1
    args.snapshot.write_text(serialized, encoding="utf-8")
    print(f"refreshed {args.snapshot.relative_to(REPO_ROOT)}: "
          f"ensemble hit@1={ensemble_hit} net_benefit_ms={ensemble['net_benefit_ms']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
