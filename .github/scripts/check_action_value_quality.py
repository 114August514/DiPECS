#!/usr/bin/env python3
"""Evaluation data quality guard.

This script blocks low-quality data from being treated as evidence of system
benefit. It is intentionally strict: if a report claims action-level
`net_benefit` or `saved_latency` without a `source` annotation that proves it
came from real device measurements, the PR is rejected.

Current enforcement target:
- data/evaluation/synthetic-next-app-v1.report.json (hard-coded constants in
  the action_value layer must not be presented as measured benefits).

Usage:
    python3 .github/scripts/check_action_value_quality.py \
        data/evaluation/synthetic-next-app-v1.report.json

Exit codes:
    0  OK
    1  Low-quality / unannotated action-value claim detected
"""

import json
import sys
from pathlib import Path

ALLOWED_SYNTHETIC_SOURCES = {
    "synthetic_constant_backtest",
    "synthetic_model_prediction_only",
}

REQUIRED_MEASURED_SOURCES = {
    "measured_device",
    "measured_emulator",
    "measured_live_trace",
}

BENEFIT_FIELDS = (
    "net_benefit_ms",
    "saved_latency_ms",
    "wasted_action_cost_ms",
    "control_plane_cost_ms",
)


def fail(message: str) -> None:
    print(f"FAIL: {message}", file=sys.stderr)
    sys.exit(1)


def check_report(path: Path) -> None:
    if not path.exists():
        # Missing report is allowed; we only guard files that are committed.
        print(f"SKIP: {path} does not exist")
        return

    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail(f"{path} is not valid JSON: {exc}")

    # Top-level source annotation is the primary gate.
    top_source = data.get("source") if isinstance(data, dict) else None
    schema = data.get("schema_version") if isinstance(data, dict) else None

    if isinstance(data, dict) and any(field in data for field in BENEFIT_FIELDS):
        # If the report itself contains benefit fields, it must declare a source.
        if top_source in ALLOWED_SYNTHETIC_SOURCES:
            print(
                f"WARN: {path} contains synthetic action-value claims "
                f"(source={top_source}); these MUST NOT be cited as measured "
                f"system benefits."
            )
            return
        if top_source not in REQUIRED_MEASURED_SOURCES:
            fail(
                f"{path} contains action-value benefit fields but source="
                f"'{top_source}' is neither measured nor an allowed synthetic "
                f"annotation. Add source=measured_device or "
                f"source=synthetic_constant_backtest with explicit documentation."
            )

    # Deep scan: any nested object containing benefit fields needs a source.
    if isinstance(data, dict) and "results" in data:
        results = data["results"]
        if isinstance(results, list):
            for idx, entry in enumerate(results):
                if not isinstance(entry, dict):
                    continue
                entry_source = entry.get("source", top_source)
                if any(field in entry for field in BENEFIT_FIELDS):
                    if entry_source in ALLOWED_SYNTHETIC_SOURCES:
                        print(
                            f"WARN: results[{idx}] in {path} contains synthetic "
                            f"action-value claims (source={entry_source}); these "
                            f"MUST NOT be cited as measured system benefits."
                        )
                        continue
                    if entry_source not in REQUIRED_MEASURED_SOURCES:
                        fail(
                            f"{path} results[{idx}] contains benefit fields but "
                            f"source='{entry_source}' is not a permitted source."
                        )

    # Schema-specific guard for the known synthetic next-app benchmark report.
    if schema == "dipecs.next_app_benchmark.v2":
        # This schema historically emitted net_benefit from hard-coded constants.
        # We allow it to exist only if every benefit value is tagged synthetic.
        synthetic_tag = data.get("action_value_source")
        if synthetic_tag not in ALLOWED_SYNTHETIC_SOURCES:
            fail(
                f"{path} uses schema {schema} which is known to derive "
                f"net_benefit from hard-coded constants. Either set "
                f"action_value_source to one of {ALLOWED_SYNTHETIC_SOURCES} or "
                f"migrate to measured device data."
            )
        print(
            f"OK: {path} is correctly tagged as synthetic "
            f"(action_value_source={synthetic_tag}); will not be treated as "
            f"measured benefit."
        )
        return

    print(f"OK: {path} passes action-value quality guard")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <report.json>", file=sys.stderr)
        sys.exit(2)

    for arg in sys.argv[1:]:
        check_report(Path(arg))
