#!/usr/bin/env python3
"""Evaluation data quality guard.

This script blocks low-quality data from being treated as evidence of system
benefit. It is intentionally strict: if a report claims action-level
`net_benefit` or `saved_latency` without provenance showing either real device
measurement or an explicit synthetic backtest, the PR is rejected.

Enforcement targets:
- Any committed evaluation JSON that contains action-value benefit fields
  (net_benefit_ms, saved_latency_ms, ...) at ANY depth must carry a `source`
  (or `action_value_source`) annotation proving the numbers are measured, or an
  explicit synthetic tag marking them as NOT a measured benefit.
- data/evaluation/synthetic-next-app-v1.report.json (hard-coded constants in
  the action_value layer must not be presented as measured benefits).

Usage:
    # Explicit files:
    python3 scripts/ci/check_action_value_quality.py \
        data/evaluation/synthetic-next-app-v1.report.json
    # Or scan an entire directory tree of *.json:
    python3 scripts/ci/check_action_value_quality.py --dir data/evaluation

Exit codes:
    0  OK
    1  Low-quality / unannotated action-value claim detected
"""

import json
import sys
from pathlib import Path

ALLOWED_SCHEMA_SOURCES = {
    "synthetic_constant_backtest",
    "synthetic_model_prediction_only",
}

ALLOWED_SYNTHETIC_BENEFIT_SOURCES = {
    "synthetic_constant_backtest",
}

REQUIRED_MEASURED_SOURCES = {
    "measured_device",
    "measured_emulator",
    "measured_live_trace",
}

BENEFIT_FIELDS = (
    "net_benefit_ms",
    "saved_latency_ms",
    "gross_saved_ms",
    "gross_wasted_ms",
    "wasted_action_cost_ms",
    "control_plane_cost_ms",
)

PROVENANCE_FIELDS = (
    "action_value_source",
    "source",
)


def fail(message: str) -> None:
    print(f"FAIL: {message}", file=sys.stderr)
    sys.exit(1)


def provenance(value: dict) -> str | None:
    for field in PROVENANCE_FIELDS:
        found = value.get(field)
        if isinstance(found, str) and found.strip():
            return found
    return None


def is_measured_source(source: str | None) -> bool:
    return source in REQUIRED_MEASURED_SOURCES


def is_synthetic_benefit_source(source: str | None) -> bool:
    return source in ALLOWED_SYNTHETIC_BENEFIT_SOURCES


def is_schema_source(source: str | None) -> bool:
    return is_measured_source(source) or source in ALLOWED_SCHEMA_SOURCES


def scan_benefit_claims(
    value,
    path: str,
    inherited_source: str | None,
    inherited_allowed: bool,
) -> int:
    """Return the number of synthetic benefit warnings emitted."""
    warnings = 0
    if isinstance(value, dict):
        local_source = provenance(value)
        active_source = local_source or (inherited_source if inherited_allowed else None)
        has_benefit = any(field in value for field in BENEFIT_FIELDS)

        if has_benefit:
            if is_synthetic_benefit_source(active_source):
                print(
                    f"WARN: {path} contains synthetic action-value claims "
                    f"(source={active_source}); these MUST NOT be cited as "
                    f"measured system benefits."
                )
                warnings += 1
            elif not is_measured_source(active_source):
                fail(
                    f"{path} contains benefit fields but source="
                    f"'{active_source}' is not permitted. Use a measured source "
                    f"({sorted(REQUIRED_MEASURED_SOURCES)}) or explicitly tag a "
                    f"synthetic backtest with source=synthetic_constant_backtest."
                )

        child_inherited_source = active_source
        child_inherited_allowed = inherited_allowed or is_schema_source(active_source)
        for key, child in value.items():
            warnings += scan_benefit_claims(
                child,
                f"{path}.{key}",
                child_inherited_source,
                child_inherited_allowed,
            )
        return warnings

    if isinstance(value, list):
        for idx, child in enumerate(value):
            warnings += scan_benefit_claims(
                child,
                f"{path}[{idx}]",
                inherited_source,
                inherited_allowed,
            )
    return warnings


def check_report(path: Path) -> None:
    if not path.exists():
        # Missing report is allowed; we only guard files that are committed.
        print(f"SKIP: {path} does not exist")
        return

    try:
        # utf-8-sig tolerates a leading BOM, which some committed fixtures carry.
        data = json.loads(path.read_text(encoding="utf-8-sig"))
    except json.JSONDecodeError as exc:
        fail(f"{path} is not valid JSON: {exc}")

    # Top-level provenance can be inherited by nested benefit records, but only
    # when it is itself a known measured or synthetic source.
    top_source = provenance(data) if isinstance(data, dict) else None
    schema = data.get("schema_version") if isinstance(data, dict) else None

    warnings = scan_benefit_claims(
        data,
        str(path),
        top_source,
        is_schema_source(top_source),
    )

    # Schema-specific guard for the known synthetic next-app benchmark report.
    if schema == "dipecs.next_app_benchmark.v2":
        # This schema is generated from deterministic synthetic traces. It is
        # allowed only with explicit action-value provenance, even when it emits
        # no benefit fields.
        synthetic_tag = data.get("action_value_source")
        if synthetic_tag not in ALLOWED_SCHEMA_SOURCES:
            fail(
                f"{path} uses schema {schema}, which is synthetic. Set "
                f"action_value_source to one of {sorted(ALLOWED_SCHEMA_SOURCES)} "
                f"or migrate to measured device data."
            )
        print(
            f"OK: {path} is correctly tagged as synthetic "
            f"(action_value_source={synthetic_tag}); will not be treated as "
            f"measured benefit."
        )
        return

    if warnings:
        print(f"OK: {path} passes with {warnings} synthetic benefit warning(s)")
        return

    print(f"OK: {path} passes action-value quality guard")


def iter_paths(args: list[str]):
    """Yield report paths from explicit files or `--dir <tree>` arguments."""
    i = 0
    while i < len(args):
        arg = args[i]
        if arg == "--dir":
            i += 1
            if i >= len(args):
                print("Usage: --dir <directory>", file=sys.stderr)
                sys.exit(2)
            root = Path(args[i])
            yield from sorted(root.rglob("*.json"))
        else:
            yield Path(arg)
        i += 1


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(
            f"Usage: {sys.argv[0]} <report.json> [more.json ...] | --dir <directory>",
            file=sys.stderr,
        )
        sys.exit(2)

    for report_path in iter_paths(sys.argv[1:]):
        check_report(report_path)
