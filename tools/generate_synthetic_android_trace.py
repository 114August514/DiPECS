#!/usr/bin/env python3
"""Generate deterministic synthetic Android collector JSONL traces.

The output is intentionally synthetic and redacted. It is useful for replay,
dashboard, policy/audit, and documentation demos when real-device traces are
not available. It must not be presented as real-device evidence.
"""

from __future__ import annotations

import argparse
import json
import random
from collections import Counter
from pathlib import Path


PACKAGES = [
    ("com.example.chat", "ChatActivity", "msg", "messages"),
    ("com.example.mail", "InboxActivity", "email", "mail"),
    ("com.example.browser", "BrowserActivity", "status", "browser_status"),
    ("com.example.docs", "DocumentActivity", "progress", "sync"),
    ("com.example.calendar", "CalendarActivity", "event", "calendar"),
    ("com.android.settings", "Settings", "status", "system"),
]

NETWORKS = ["Wifi", "Cellular", "None"]
RINGER_MODES = ["Normal", "Vibrate", "Silent"]
ACCESSIBILITY_TYPES = [
    ("view_clicked", "android.widget.Button"),
    ("view_focused", "android.widget.EditText"),
    ("window_state_changed", "android.widget.FrameLayout"),
    ("view_selected", "android.widget.ListView"),
]
ACTION_EVENTS = [
    ("prefetch_started", "PrefetchFile queued", {"targetKind": "url", "target": None}),
    ("prefetch_succeeded", "PrefetchFile completed", {"targetKind": "url", "bytes": 16384}),
    ("release_memory_completed", "ReleaseMemory completed", {"target": None, "deletedFiles": 3}),
    ("keep_alive_scheduled", "KeepAlive scheduled", {"target": None, "jobId": 4632101}),
    ("keep_alive_job_executed", "KeepAlive job executed", {"target": None}),
    ("own_resources_prewarmed", "PreWarmProcess completed", {"target": None}),
    ("user_visible_action_posted", "User-visible action hint posted", {"target": None}),
]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--rows",
        type=int,
        default=2400,
        help="Number of JSONL rows to generate.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("data/traces/android_synthetic_large.redacted.jsonl"),
        help="Output JSONL path.",
    )
    parser.add_argument(
        "--summary",
        type=Path,
        default=Path("data/traces/android_synthetic_large.redacted.summary.json"),
        help="Output summary JSON path.",
    )
    parser.add_argument(
        "--seed",
        type=int,
        default=20260628,
        help="Deterministic random seed.",
    )
    args = parser.parse_args()

    if args.rows < 100:
        raise SystemExit("--rows must be at least 100 for a useful synthetic trace")

    rng = random.Random(args.seed)
    base_ms = 1_718_000_000_000
    counters: Counter[str] = Counter()
    raw_kinds: Counter[str] = Counter()
    source_counts: Counter[str] = Counter()

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.summary.parent.mkdir(parents=True, exist_ok=True)

    rows = []
    foreground_package = PACKAGES[0]
    battery = 87
    charging = False

    for index in range(args.rows):
        timestamp_ms = base_ms + index * rng.randint(850, 2_750)
        event_id = f"android-synth-{index + 1:06d}"
        bucket = index % 24

        if bucket in {0, 1, 8, 12, 16, 20}:
            foreground_package = PACKAGES[(index // 4) % len(PACKAGES)]
            row = app_transition_row(event_id, timestamp_ms, foreground_package, "Foreground")
        elif bucket in {2, 9, 17, 21}:
            row = notification_posted_row(event_id, timestamp_ms, rng, foreground_package)
        elif bucket in {3, 10, 18}:
            row = notification_interaction_row(event_id, timestamp_ms, foreground_package, rng)
        elif bucket in {4, 11, 19}:
            battery = max(8, min(96, battery + rng.choice([-4, -3, -2, -1, 1, 2, 3])))
            if index % 240 == 0:
                charging = not charging
            row = system_state_row(event_id, timestamp_ms, battery, charging, rng)
        elif bucket in {5, 13, 22}:
            row = accessibility_screening_row(event_id, timestamp_ms, foreground_package, rng)
        elif bucket in {6, 14}:
            row = screen_state_row(event_id, timestamp_ms, rng.choice(["Interactive", "NonInteractive"]))
        elif bucket in {7, 15, 23}:
            row = action_trace_row(event_id, timestamp_ms, rng)
        else:
            row = app_transition_row(event_id, timestamp_ms, foreground_package, "Background")

        rows.append(row)
        counters[row["eventType"]] += 1
        source_counts[row["source"]] += 1
        raw_kind = raw_event_kind(row)
        if raw_kind is None:
            counters["rawEvent:null"] += 1
        else:
            counters["rawEvent:nonNull"] += 1
            raw_kinds[raw_kind] += 1

    with args.output.open("w", encoding="utf-8", newline="\n") as handle:
        for row in rows:
            handle.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")))
            handle.write("\n")

    summary = {
        "label": "synthetic_android_redacted_trace",
        "synthetic": True,
        "redacted": True,
        "rows": args.rows,
        "seed": args.seed,
        "output": str(args.output).replace("\\", "/"),
        "eventTypeCounts": dict(sorted(counters.items())),
        "sourceCounts": dict(sorted(source_counts.items())),
        "rawEventKindCounts": dict(sorted(raw_kinds.items())),
        "notes": [
            "Generated data is synthetic and must not be reported as real-device evidence.",
            "Notification title/text, accessibility text, socket tokens, cache paths, and action targets are redacted.",
            "Rows with rawEvent:null model screening/internal rows and are skipped by Rust production ingress.",
        ],
    }
    args.summary.write_text(
        json.dumps(summary, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
        newline="\n",
    )


def base_row(event_id: str, timestamp_ms: int, source: str, event_type: str) -> dict:
    return {
        "eventId": event_id,
        "timestampMs": timestamp_ms,
        "source": source,
        "eventType": event_type,
        "packageName": None,
        "className": None,
        "windowTitle": None,
        "text": None,
        "action": None,
        "deviceContext": None,
        "rawEvent": None,
        "rawPayload": {},
        "synthetic": True,
        "redacted": True,
    }


def app_transition_row(event_id: str, timestamp_ms: int, app: tuple[str, str, str, str], transition: str) -> dict:
    package_name, activity_class, _, _ = app
    row = base_row(event_id, timestamp_ms, "UsageCollector", "app_transition")
    row["packageName"] = package_name
    row["className"] = activity_class
    row["rawEvent"] = {
        "AppTransition": {
            "timestamp_ms": timestamp_ms,
            "package_name": package_name,
            "activity_class": activity_class,
            "transition": transition,
        }
    }
    return row


def notification_posted_row(
    event_id: str,
    timestamp_ms: int,
    rng: random.Random,
    app: tuple[str, str, str, str],
) -> dict:
    package_name, _, category, channel_id = app
    row = base_row(event_id, timestamp_ms, "NotificationCollectorService", "notification_posted")
    row["packageName"] = package_name
    row["rawEvent"] = {
        "NotificationPosted": {
            "timestamp_ms": timestamp_ms,
            "package_name": package_name,
            "category": category,
            "channel_id": channel_id,
            "raw_title": "",
            "raw_text": "",
            "is_ongoing": rng.random() < 0.07,
            "group_key": None,
            "has_picture": rng.random() < 0.05,
        }
    }
    row["rawPayload"] = {"notificationKey": None, "rankingPresent": True}
    return row


def notification_interaction_row(
    event_id: str,
    timestamp_ms: int,
    app: tuple[str, str, str, str],
    rng: random.Random,
) -> dict:
    package_name, _, _, _ = app
    action = rng.choice(["Tapped", "Dismissed", "Removed"])
    row = base_row(event_id, timestamp_ms, "NotificationCollectorService", "notification_interaction")
    row["packageName"] = package_name
    row["action"] = action
    row["rawEvent"] = {
        "NotificationInteraction": {
            "timestamp_ms": timestamp_ms,
            "package_name": package_name,
            "notification_key": "",
            "action": action,
        }
    }
    row["rawPayload"] = {"key": None}
    return row


def system_state_row(
    event_id: str,
    timestamp_ms: int,
    battery: int,
    charging: bool,
    rng: random.Random,
) -> dict:
    network = rng.choice(NETWORKS)
    ringer_mode = rng.choice(RINGER_MODES)
    row = base_row(event_id, timestamp_ms, "device_context", "context_heartbeat")
    row["deviceContext"] = {
        "timezone": "Asia/Shanghai",
        "batteryPercent": battery,
        "isCharging": charging,
        "networkType": network.lower(),
        "isScreenOn": True,
        "ringerMode": ringer_mode.lower(),
        "doNotDisturbMode": None,
    }
    row["rawEvent"] = {
        "SystemState": {
            "timestamp_ms": timestamp_ms,
            "battery_pct": battery,
            "is_charging": charging,
            "network": network,
            "ringer_mode": ringer_mode,
            "location_type": "Unknown",
            "headphone_connected": rng.random() < 0.18,
            "bluetooth_connected": rng.random() < 0.22,
        }
    }
    row["rawPayload"] = {"serviceRunning": True}
    return row


def accessibility_screening_row(
    event_id: str,
    timestamp_ms: int,
    app: tuple[str, str, str, str],
    rng: random.Random,
) -> dict:
    package_name, _, _, _ = app
    event_type, class_name = rng.choice(ACCESSIBILITY_TYPES)
    row = base_row(event_id, timestamp_ms, "AccessibilityCollectorService", event_type)
    row["packageName"] = package_name
    row["className"] = class_name
    row["rawEvent"] = None
    row["rawPayload"] = {
        "eventTypeName": event_type,
        "sourceText": None,
        "sourceContentDescription": None,
        "textItems": None,
    }
    return row


def screen_state_row(event_id: str, timestamp_ms: int, state: str) -> dict:
    row = base_row(event_id, timestamp_ms, "CollectorForegroundService", "screen_state")
    row["rawEvent"] = {
        "ScreenState": {
            "timestamp_ms": timestamp_ms,
            "state": state,
        }
    }
    return row


def action_trace_row(event_id: str, timestamp_ms: int, rng: random.Random) -> dict:
    event_type, text, payload = rng.choice(ACTION_EVENTS)
    row = base_row(event_id, timestamp_ms, "ActionExecutorBridge", event_type)
    row["text"] = text
    row["rawPayload"] = {
        **payload,
        "reason": "synthetic_replay",
        "authToken": None,
        "cachePath": None,
    }
    if event_type == "keep_alive_job_executed":
        row["rawEvent"] = {
            "SystemState": {
                "timestamp_ms": timestamp_ms,
                "battery_pct": 72,
                "is_charging": False,
                "network": "Wifi",
                "ringer_mode": "Normal",
                "location_type": "Unknown",
                "headphone_connected": False,
                "bluetooth_connected": False,
            }
        }
    return row


def raw_event_kind(row: dict) -> str | None:
    raw_event = row.get("rawEvent")
    if not isinstance(raw_event, dict) or not raw_event:
        return None
    return next(iter(raw_event))


if __name__ == "__main__":
    main()
