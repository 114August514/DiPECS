#!/usr/bin/env python3
"""Generate DeepSeek cloud-accuracy cases from public mobile-related datasets.

Supported source kinds:
- lsapp: LSApp-shaped sequential app usage data.
- mobilerec: MobileRec-shaped user/app interaction data.
- battery: smartphone battery/app/network telemetry.
- mobile-usage: aggregate mobile usage/persona datasets.

All outputs use `dipecs.cloud_llm_accuracy_cases.v1` and contain only sanitized
features: package-like identifiers, coarse semantic hints, system status, and
expected DiPECS decisions. No raw notification text is generated.
"""

from __future__ import annotations

import argparse
import csv
import json
import random
from collections import defaultdict, deque
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


APP_TAXONOMY = [
    ("chat", ["chat", "message", "messenger", "whatsapp", "wechat", "telegram", "line", "slack", "teams"]),
    ("mail", ["mail", "gmail", "outlook", "email"]),
    ("calendar", ["calendar", "agenda", "meeting"]),
    ("browser", ["browser", "chrome", "firefox", "safari", "edge"]),
    ("map", ["map", "maps", "navigation", "uber", "lyft"]),
    ("music", ["music", "spotify", "audio", "podcast"]),
    ("gallery", ["photo", "gallery", "camera", "image", "instagram"]),
    ("docs", ["doc", "drive", "office", "word", "excel", "sheet", "pdf"]),
    ("finance", ["bank", "finance", "pay", "wallet", "alipay", "paypal"]),
    ("social", ["facebook", "twitter", "x", "social", "reddit", "weibo"]),
    ("news", ["news", "reader", "feed"]),
    ("video", ["video", "youtube", "netflix", "tiktok"]),
    ("game", ["game"]),
    ("code", ["github", "gitlab", "ide", "code", "dev"]),
]

CATEGORY_TAXONOMY = {
    "communication": "chat",
    "social": "social",
    "productivity": "docs",
    "business": "docs",
    "finance": "finance",
    "tools": "other",
    "education": "docs",
    "maps": "map",
    "travel": "map",
    "music": "music",
    "audio": "music",
    "photography": "gallery",
    "video": "video",
    "entertainment": "video",
    "news": "news",
    "game": "game",
    "games": "game",
    "developer": "code",
}

PACKAGE_BY_KIND = {
    "chat": "com.example.chat",
    "mail": "com.example.mail",
    "calendar": "com.example.calendar",
    "browser": "com.example.browser",
    "map": "com.example.maps",
    "music": "com.example.music",
    "gallery": "com.example.gallery",
    "docs": "com.example.docs",
    "finance": "com.example.finance",
    "social": "com.example.social",
    "news": "com.example.news",
    "video": "com.example.video",
    "game": "com.example.game",
    "code": "com.example.devops",
    "other": "com.example.app",
}

PERSONAS = {
    "chat": "heavy messenger user, reacts to direct messages and shared attachments",
    "mail": "office worker, triages email and document attachments",
    "calendar": "schedule-driven user, checks meeting reminders",
    "browser": "reader/researcher, follows useful links",
    "map": "commuter, reacts to navigation and travel updates",
    "music": "media listener, keeps playback sessions stable",
    "gallery": "visual creator, opens image media quickly",
    "docs": "knowledge worker, prepares documents and spreadsheets",
    "finance": "privacy-sensitive finance user, checks money alerts manually",
    "social": "social media user, checks mention and media notifications",
    "news": "news reader, opens article links",
    "video": "video watcher/editor, opens video media",
    "game": "gamer, avoids low-value background actions while playing",
    "code": "developer, checks build and code artifact notifications",
    "other": "general smartphone user, prefers conservative actions without strong signals",
}


@dataclass
class Record:
    user_id: str
    session_id: str
    timestamp_ms: int
    app_name: str
    event_type: str
    ordinal: int


def normalize_kind(app_name: str, category: str | None = None) -> str:
    if category:
        cat = category.lower().strip()
        for needle, kind in CATEGORY_TAXONOMY.items():
            if needle in cat:
                return kind
    lower = app_name.lower()
    for kind, needles in APP_TAXONOMY:
        if any(needle in lower for needle in needles):
            return kind
    return "other"


def package_for(app_name: str, category: str | None = None) -> str:
    return PACKAGE_BY_KIND[normalize_kind(app_name, category)]


def semantic_hints_for(kind: str, next_kind: str, rng: random.Random) -> list[str]:
    if next_kind in {"docs", "mail"}:
        return ["FileMention"]
    if next_kind == "gallery":
        return ["ImageMention"]
    if next_kind == "music":
        return ["AudioMessage"]
    if next_kind in {"browser", "news", "map"}:
        return ["LinkAttachment"]
    if next_kind == "calendar":
        return ["CalendarInvitation"]
    if next_kind == "finance":
        return ["FinancialContext"]
    if next_kind in {"chat", "social"}:
        return ["UserMentioned"] if rng.random() < 0.45 else []
    if next_kind == "code":
        return ["FileMention"]
    return []


def file_activity_for(next_kind: str) -> list[list[Any]]:
    if next_kind in {"docs", "mail"}:
        return [["Document", 1]]
    if next_kind == "gallery":
        return [["Image", 1]]
    if next_kind == "music":
        return [["Audio", 1]]
    if next_kind == "video":
        return [["Video", 1]]
    if next_kind == "code":
        return [["Code", 1], ["Archive", 1]]
    return []


def expected_for(next_kind: str, next_pkg: str, hints: list[str], label_mode: str = "hinted-action") -> list[dict[str, Any]]:
    if label_mode == "next-app":
        if next_kind in {"game", "other"}:
            return [{"intent_type": "Idle", "action_type": "NoOp"}]
        return [{"intent_type": "OpenApp", "target": next_pkg, "action_type": "PreWarmProcess"}]

    if label_mode == "action-intent":
        if next_kind in {"game", "other"}:
            return [{"action_type": "NoOp"}]
        return [{"action_type": "PreWarmProcess"}]

    files = file_activity_for(next_kind)
    if files:
        return [{"intent_type": "HandleFile", "extension_category": files[0][0], "action_type": "PrefetchFile"}]
    if next_kind in {"game", "other"} and not hints:
        return [{"intent_type": "Idle", "action_type": "NoOp"}]
    return [{"intent_type": "CheckNotification", "target": next_pkg, "action_type": "PreWarmProcess"}]


def input_files(path: Path) -> list[Path]:
    return [path] if path.is_file() else sorted(
        p for p in path.rglob("*") if p.suffix.lower() in {".tsv", ".csv", ".jsonl"}
    )


def iter_rows(path: Path, limit_rows: int | None = None) -> Iterable[dict[str, Any]]:
    seen = 0
    for file in input_files(path):
        if file.suffix.lower() == ".jsonl":
            with file.open("r", encoding="utf-8", errors="replace") as f:
                for line in f:
                    if limit_rows and seen >= limit_rows:
                        return
                    if not line.strip():
                        continue
                    row = json.loads(line)
                    if isinstance(row, dict):
                        seen += 1
                        yield row
        else:
            with file.open("r", encoding="utf-8", errors="replace", newline="") as f:
                sample = f.readline()
                if not sample:
                    continue
                delim = "\t" if "\t" in sample else ","
                f.seek(0)
                reader = csv.DictReader(f, delimiter=delim)
                for row in reader:
                    if limit_rows and seen >= limit_rows:
                        return
                    seen += 1
                    yield dict(row)


def pick(row: dict[str, Any], names: Iterable[str]) -> str | None:
    lowered = {str(k).lower().strip(): v for k, v in row.items()}
    for name in names:
        value = lowered.get(name)
        if value is not None and str(value).strip():
            return str(value).strip()
    return None


def parse_ts(raw: str | None, ordinal: int) -> int:
    if raw is None:
        return ordinal * 1000
    try:
        value = int(float(raw))
    except ValueError:
        return ordinal * 1000
    return value if value >= 1_000_000_000_000 else value * 1000


def record_from_mapping(row: dict[str, Any], ordinal: int) -> Record | None:
    user = pick(row, ["user_id", "userid", "user", "uid", "user id"])
    app = pick(row, ["app_name", "appname", "app", "package", "package_name", "app_package", "item", "item_id"])
    if not user or not app:
        return None
    return Record(
        user_id=user,
        session_id=pick(row, ["session_id", "sessionid", "session"]) or "default",
        timestamp_ms=parse_ts(pick(row, ["timestamp_ms", "timestamp", "unix_timestamp", "time", "ts", "date"]), ordinal),
        app_name=app,
        event_type=pick(row, ["event_type", "event", "type"]) or "app_usage",
        ordinal=ordinal,
    )


def read_records(path: Path, limit_rows: int | None) -> list[Record]:
    records = []
    for ordinal, row in enumerate(iter_rows(path, limit_rows)):
        rec = record_from_mapping(row, ordinal)
        if rec:
            records.append(rec)
    return records


def build_sequence_cases(records: list[Record], max_cases: int, horizon_secs: int, history_len: int, seed: int, source_name: str, label_mode: str) -> list[dict[str, Any]]:
    rng = random.Random(seed)
    by_user: dict[str, list[Record]] = defaultdict(list)
    for rec in records:
        by_user[rec.user_id].append(rec)

    candidates = []
    for user_id, rows in by_user.items():
        rows.sort(key=lambda r: (r.session_id, r.timestamp_ms, r.ordinal))
        history: deque[str] = deque(maxlen=history_len)
        for idx, current in enumerate(rows[:-1]):
            next_rec = None
            for cand in rows[idx + 1 :]:
                if cand.session_id != current.session_id:
                    break
                if cand.timestamp_ms - current.timestamp_ms > horizon_secs * 1000:
                    break
                if cand.app_name != current.app_name:
                    next_rec = cand
                    break
            if next_rec:
                candidates.append((user_id, list(history), current, next_rec))
            history.append(current.app_name)

    rng.shuffle(candidates)
    cases = []
    for idx, (user_id, history, current, next_rec) in enumerate(candidates[:max_cases], start=1):
        current_kind = normalize_kind(current.app_name)
        next_kind = normalize_kind(next_rec.app_name)
        current_pkg = package_for(current.app_name)
        next_pkg = package_for(next_rec.app_name)
        if label_mode in {"next-app", "action-intent"}:
            hints = []
            notified_apps = []
            file_activity = []
            persona_kind = current_kind
            task = "predict the next app category" if label_mode == "next-app" else "predict whether a resource optimization action is useful"
            scenario = f"{task} from recent {current_kind} usage without future notification hints"
            behavior_profile = {
                "recent_app_categories": [normalize_kind(app) for app in history],
                "current_app_category": current_kind,
                "history_len": len(history),
                "prediction_horizon_secs": horizon_secs,
            }
        else:
            hints = semantic_hints_for(current_kind, next_kind, rng)
            notified_apps = [] if next_kind in {"game", "other"} else [next_pkg]
            file_activity = file_activity_for(next_kind)
            persona_kind = next_kind
            scenario = f"after {current_kind} usage, generated future hint category is {next_kind}"
            behavior_profile = {
                "recent_app_categories": [normalize_kind(app) for app in history],
                "current_app_category": current_kind,
                "generated_label_category": next_kind,
            }
        cases.append({
            "id": f"{source_name.lower()}_{idx:05d}",
            "source_dataset": source_name,
            "source_user_id": user_id,
            "label_mode": label_mode,
            "persona": PERSONAS.get(persona_kind, PERSONAS["other"]),
            "scenario": scenario,
            "foreground_apps": [current_pkg],
            "notified_apps": notified_apps,
            "semantic_hints": hints,
            "file_activity": file_activity,
            "behavior_profile": behavior_profile,
            "expected": expected_for(next_kind, next_pkg, hints, label_mode),
        })
    return cases


def build_mobilerec_cases(path: Path, max_cases: int, limit_rows: int | None, seed: int) -> list[dict[str, Any]]:
    rows = list(iter_rows(path, limit_rows))
    rng = random.Random(seed)
    rng.shuffle(rows)
    cases = []
    for row in rows:
        if len(cases) >= max_cases:
            break
        app = pick(row, ["app_package", "package", "package_name", "app_name", "app", "item", "item_id"])
        if not app:
            continue
        category = pick(row, ["app_category", "category", "genre", "app_genre"])
        kind = normalize_kind(app, category)
        pkg = package_for(app, category)
        hints = semantic_hints_for("other", kind, rng)
        cases.append({
            "id": f"mobilerec_{len(cases) + 1:05d}",
            "source_dataset": "MobileRec",
            "source_user_id": pick(row, ["uid", "user_id", "userid", "user"]),
            "persona": PERSONAS.get(kind, PERSONAS["other"]),
            "scenario": f"user has a relevant app interaction in category {kind}",
            "foreground_apps": ["com.example.home"],
            "notified_apps": [] if kind in {"game", "other"} else [pkg],
            "semantic_hints": hints,
            "file_activity": file_activity_for(kind),
            "behavior_profile": {"app_category": category, "app_name_or_package": app},
            "expected": expected_for(kind, pkg, hints),
        })
    return cases


def parse_float(raw: str | None, default: float = 0.0) -> float:
    if raw is None:
        return default
    try:
        return float(str(raw).strip().replace("%", ""))
    except ValueError:
        return default


def build_battery_cases(path: Path, max_cases: int, limit_rows: int | None) -> list[dict[str, Any]]:
    cases = []
    for row in iter_rows(path, limit_rows):
        if len(cases) >= max_cases:
            break
        battery = parse_float(pick(row, ["battery_pct", "battery", "battery_level", "battery percentage", "battery_percent"]), 50.0)
        charging_raw = (pick(row, ["is_charging", "charging", "plugged"]) or "false").lower()
        is_charging = charging_raw in {"1", "true", "yes", "charging", "plugged"}
        network_raw = (pick(row, ["network", "network_type", "connectivity"]) or "Unknown").lower()
        network = "Wifi" if "wifi" in network_raw else "Cellular" if "cell" in network_raw or "mobile" in network_raw else "Offline" if "offline" in network_raw or "none" in network_raw else "Unknown"
        foreground = package_for(pick(row, ["app", "app_name", "foreground_app", "package", "package_name"]) or "home")
        if battery <= 15 and not is_charging:
            expected = [{"intent_type": "Idle", "action_type": "ReleaseMemory"}]
            scenario = "low battery and not charging"
        elif is_charging and network == "Wifi":
            expected = [{"intent_type": "Idle", "action_type": "KeepAlive"}]
            scenario = "charging on wifi"
        elif network == "Offline":
            expected = [{"intent_type": "Idle", "action_type": "NoOp"}]
            scenario = "offline state avoids network-heavy actions"
        else:
            continue
        cases.append({
            "id": f"battery_{len(cases) + 1:05d}",
            "source_dataset": "battery-context",
            "persona": "system-conscious smartphone user",
            "scenario": scenario,
            "foreground_apps": [foreground],
            "notified_apps": [],
            "semantic_hints": [],
            "system_status": {
                "battery_pct": int(max(0, min(100, round(battery)))),
                "is_charging": is_charging,
                "network": network,
                "ringer_mode": "Normal",
                "location_type": "Unknown",
            },
            "expected": expected,
        })
    return cases


def build_mobile_usage_cases(path: Path, max_cases: int, limit_rows: int | None, seed: int) -> list[dict[str, Any]]:
    rng = random.Random(seed)
    rows = list(iter_rows(path, limit_rows))
    rng.shuffle(rows)
    cases = []
    for row in rows:
        if len(cases) >= max_cases:
            break
        usage = " ".join(str(v).lower() for v in row.values() if v is not None)
        if "gaming" in usage or "game" in usage:
            kind = "game"
        elif "stream" in usage or "video" in usage:
            kind = "video"
        elif "social" in usage:
            kind = "social"
        elif "work" in usage or "product" in usage:
            kind = "docs"
        else:
            kind = "other"
        pkg = PACKAGE_BY_KIND[kind]
        hints = semantic_hints_for("other", kind, rng)
        cases.append({
            "id": f"mobile_usage_{len(cases) + 1:05d}",
            "source_dataset": "mobile-usage",
            "persona": PERSONAS.get(kind, PERSONAS["other"]),
            "scenario": f"aggregate mobile usage profile indicates {kind} behavior",
            "foreground_apps": [pkg],
            "notified_apps": [] if kind in {"game", "other"} else [pkg],
            "semantic_hints": hints,
            "file_activity": file_activity_for(kind),
            "expected": expected_for(kind, pkg, hints),
        })
    return cases


def load_seed(path: Path) -> list[dict[str, Any]]:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f).get("cases", [])


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-kind", choices=["lsapp", "mobilerec", "battery", "mobile-usage"], default="lsapp")
    parser.add_argument("--input", default="data/lsapp/lsapp.tsv", help="input file or directory")
    parser.add_argument("--seed-cases", default="data/evaluation/cloud/cloud-llm-accuracy-cases.json")
    parser.add_argument("--output", default="data/evaluation/cloud/cloud-llm-accuracy-cases.generated.json")
    parser.add_argument("--max-cases", type=int, default=500)
    parser.add_argument("--limit-rows", type=int)
    parser.add_argument("--horizon-secs", type=int, default=30)
    parser.add_argument("--history-len", type=int, default=5)
    parser.add_argument("--seed", type=int, default=20260703)
    parser.add_argument(
        "--label-mode",
        choices=["action-intent", "next-app", "hinted-action"],
        default="action-intent",
        help="action-intent scores optimization action type without future hints; next-app scores target app prediction; hinted-action keeps legacy synthetic cues",
    )
    parser.add_argument("--include-seed", action="store_true", help="prepend hand-written seed cases")
    args = parser.parse_args()

    source = Path(args.input)
    if not source.exists():
        raise SystemExit(f"{source} not found. Prepare/download the selected source dataset first.")

    if args.source_kind == "lsapp":
        records = read_records(source, args.limit_rows)
        cases = build_sequence_cases(records, args.max_cases, args.horizon_secs, args.history_len, args.seed, "LSApp", args.label_mode)
        record_count = len(records)
    elif args.source_kind == "mobilerec":
        cases = build_mobilerec_cases(source, args.max_cases, args.limit_rows, args.seed)
        record_count = args.limit_rows or "unknown"
    elif args.source_kind == "battery":
        cases = build_battery_cases(source, args.max_cases, args.limit_rows)
        record_count = args.limit_rows or "unknown"
    else:
        cases = build_mobile_usage_cases(source, args.max_cases, args.limit_rows, args.seed)
        record_count = args.limit_rows or "unknown"

    if args.include_seed:
        cases = load_seed(Path(args.seed_cases)) + cases

    doc = {
        "schema_version": "dipecs.cloud_llm_accuracy_cases.v1",
        "description": f"Generated labeled cloud LLM accuracy cases from {args.source_kind}. Contains no raw notification text.",
        "source": {
            "kind": args.source_kind,
            "input": str(source),
            "max_cases": args.max_cases,
            "horizon_secs": args.horizon_secs,
            "history_len": args.history_len,
            "seed": args.seed,
            "label_mode": args.label_mode,
        },
        "cases": cases,
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {out} cases={len(cases)} records={record_count}")


if __name__ == "__main__":
    main()