#!/usr/bin/env python3
"""Convert DiPECS Android trace JSONL into an LSApp-shaped next-app CSV.

The output is intentionally small and simple so it can be consumed by:

  cargo run -p aios-cli -- train-next-app --input <csv> ...
  cargo run -p aios-cli -- eval-next-app --input <csv> ...

Only foreground AppTransition raw events are used. Consecutive duplicate
packages are collapsed because they do not form meaningful next-app labels.
"""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path


def iter_transitions(path: Path):
    previous_package = None
    with path.open("r", encoding="utf-8") as handle:
        for ordinal, line in enumerate(handle):
            line = line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            app_transition = (event.get("rawEvent") or {}).get("AppTransition") or {}
            package_name = (
                app_transition.get("package_name")
                or app_transition.get("to_package")
                or event.get("packageName")
                or ""
            ).strip()
            transition = (app_transition.get("transition") or "Foreground").strip().lower()
            if not package_name or transition not in {"", "foreground"}:
                continue
            if package_name == previous_package:
                continue
            previous_package = package_name
            timestamp_ms = (
                app_transition.get("timestamp_ms")
                or event.get("timestampMs")
                or ordinal * 1000
            )
            try:
                timestamp_ms = int(timestamp_ms)
            except (TypeError, ValueError):
                timestamp_ms = ordinal * 1000
            yield {
                "user_id": "real_device",
                "session_id": "phone_session",
                "timestamp_ms": timestamp_ms,
                "app_name": package_name,
                "event_type": "foreground",
            }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    rows = list(iter_transitions(args.input))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=["user_id", "session_id", "timestamp_ms", "app_name", "event_type"],
        )
        writer.writeheader()
        writer.writerows(rows)

    print(f"wrote {len(rows)} foreground transitions -> {args.output}")
    if len(rows) < 30:
        print("warning: fewer than 30 transitions; accuracy will be very noisy")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
