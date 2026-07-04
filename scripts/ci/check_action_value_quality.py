#!/usr/bin/env python3
"""Evaluation data quality guard.

This script blocks low-quality data from being treated as evidence of system
benefit. It is intentionally strict: if a report claims action-level
`net_benefit` or `saved_latency` without a `source` annotation that proves it
came from real device measurements, the PR is rejected.

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
    "gross_saved_ms",
    "gross_wasted_ms",
    "wasted_action_cost_ms",
    "control_plane_cost_ms",
)


def fail(message: str) -> None:
    print(f"FAIL: {message}", file=sys.stderr)
    sys.exit(1)


def _annotation_of(node: dict, inherited: str | None) -> str | None:
    """Resolve the source annotation for a node, inheriting from ancestors."""
    return node.get("source") or node.get("action_value_source") or inherited


def _walk_benefit_nodes(path: Path, node, inherited: str | None, trail: str) -> None:
    """Recursively verify every object carrying benefit fields is annotated.

    A node anywhere in the tree that contains any BENEFIT_FIELDS must resolve
    (via itself or an ancestor) to an allowed synthetic tag or a measured
    source. Unannotated benefit numbers fail the guard at any depth.
    """
    if isinstance(node, dict):
        source = _annotation_of(node, inherited)
        if any(field in node for field in BENEFIT_FIELDS):
            if source in ALLOWED_SYNTHETIC_SOURCES:
                print(
                    f"WARN: {path}:{trail or '<root>'} has synthetic action-value "
                    f"claims (source={source}); MUST NOT be cited as measured."
                )
            elif source not in REQUIRED_MEASURED_SOURCES:
                fail(
                    f"{path}:{trail or '<root>'} contains action-value benefit "
                    f"fields but source='{source}' is neither a measured source "
                    f"{sorted(REQUIRED_MEASURED_SOURCES)} nor an allowed synthetic "
                    f"tag {sorted(ALLOWED_SYNTHETIC_SOURCES)}. Annotate it."
                )
        for key, value in node.items():
            _walk_benefit_nodes(path, value, source, f"{trail}.{key}" if trail else key)
    elif isinstance(node, list):
        for idx, item in enumerate(node):
            _walk_benefit_nodes(path, item, inherited, f"{trail}[{idx}]")


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

    schema = data.get("schema_version") if isinstance(data, dict) else None

    # Recursive scan: any object carrying benefit fields at any depth must be
    # annotated (via itself or an ancestor). This catches nested metrics blocks
    # that a top-level-only check would miss.
    _walk_benefit_nodes(path, data, None, "")

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
