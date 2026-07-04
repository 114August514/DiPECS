#!/usr/bin/env python3
"""Evaluate live cloud LLM top-k accuracy over DiPECS accuracy cases."""

from __future__ import annotations

import argparse
import json
import os
import time
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib import request


DEFAULT_PROMPT = """You are the decision backend for DiPECS.
Return only valid JSON with this shape:
{"intents":[{"intent_type":"OpenApp|SwitchToApp|CheckNotification|HandleFile|EnterContext|Idle","target":"optional string","extension_category":"Document|Image|Video|Audio|Archive|Code|Other|Unknown","confidence":0.0,"risk_level":"Low|Medium|High","actions":[{"action_type":"PreWarmProcess|PrefetchFile|KeepAlive|ReleaseMemory|NoOp","target":"optional string","urgency":"Immediate|IdleTime|Deferred"}],"rationale_tags":["short_tag"]}]}
Rules:
- Return JSON only, no markdown fences.
- Use at most 3 intents.
- If uncertain, return one Idle intent with one NoOp action.
- Intent type selection:
  * HandleFile: when file_activity is present or notification references a file.
  * CheckNotification: for notifications (especially with semantic_hints like
    LinkAttachment, FinancialContext, VerificationCode, FileMention).
  * OpenApp/SwitchToApp: for foreground app transitions.
  * Idle: when no actionable signal exists.
- Action selection based on semantic hints and context:
  * PreWarmProcess when notified_apps have semantic_hints (LinkAttachment,
    FinancialContext, VerificationCode, FileMention) — user will likely
    switch to that app soon.
  * PrefetchFile when file_activity is present with a specific extension.
  * ReleaseMemory when battery is low and not charging.
  * KeepAlive when charging or when system needs sustained background work.
  * NoOp only when there is genuinely no actionable signal.
- For PrefetchFile targets, use the app package name (e.g. `com.example.chat`)
  or a concrete Android bridge target (`url:https://...`, `uri:content://...`).
- For PreWarmProcess, use `own:resources` or `pkg:<observed.package>`.
- For KeepAlive, use `work:collector_heartbeat`.
- For ReleaseMemory, use `cache:prefetch`.
- Use short snake_case rationale tags.
- The user message contains `model_input_json` with:
  - `current_context`: the current sanitized window (events, summary).
  - `behavior_profile`: long-running privacy-preserving habit summary.
  - `recent_feedback`: recent decisions plus local policy/execution outcomes.
- Prefer current_context for immediate facts, use behavior_profile for stable
  tendencies, and use recent_feedback to avoid repeating recently denied,
  failed, or low-value actions.
"""


def load_dotenv(path: Path) -> None:
    if not path.exists():
        return
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in ("\'", "\""):
            value = value[1:-1]
        os.environ.setdefault(key.strip(), value)


def build_context(case: dict[str, Any]) -> dict[str, Any]:
    hints = case.get("semantic_hints", [])
    events = []
    ts = 1_718_000_000_000
    for idx, app in enumerate(case.get("foreground_apps", [])):
        events.append({
            "event_id": f"{case['id']}-fg-{idx}",
            "timestamp_ms": ts + idx * 1000,
            "event_type": {"AppTransition": {"package_name": app, "activity_class": None, "transition": "Foreground"}},
            "source_tier": "PublicApi",
            "app_package": app,
            "uid": None,
        })
    for idx, app in enumerate(case.get("notified_apps", [])):
        events.append({
            "event_id": f"{case['id']}-notif-{idx}",
            "timestamp_ms": ts + 10_000 + idx * 1000,
            "event_type": {"Notification": {
                "source_package": app,
                "category": "msg",
                "channel_id": None,
                "title_hint": {"length_chars": 24, "script": "Latin", "is_emoji_only": False},
                "text_hint": {"length_chars": 80, "script": "Latin", "is_emoji_only": False},
                "semantic_hints": hints,
                "is_ongoing": False,
                "group_key": None,
            }},
            "source_tier": "PublicApi",
            "app_package": app,
            "uid": None,
        })
    for idx, pair in enumerate(case.get("file_activity", [])):
        category, count = pair
        events.append({
            "event_id": f"{case['id']}-file-{idx}",
            "timestamp_ms": ts + 20_000 + idx * 1000,
            "event_type": {"FileActivity": {
                "package_name": (case.get("notified_apps") or case.get("foreground_apps") or [None])[0],
                "extension_category": category,
                "activity_type": "Read",
                "is_hot_file": True,
            }},
            "source_tier": "PublicApi",
            "app_package": (case.get("notified_apps") or case.get("foreground_apps") or [None])[0],
            "uid": None,
        })
    latest = case.get("system_status")
    if latest:
        events.append({
            "event_id": f"{case['id']}-system",
            "timestamp_ms": ts + 30_000,
            "event_type": {"SystemStatus": {
                "battery_pct": latest.get("battery_pct"),
                "is_charging": latest.get("is_charging", False),
                "network": latest.get("network", "Unknown"),
                "ringer_mode": latest.get("ringer_mode", "Normal"),
                "location_type": latest.get("location_type", "Unknown"),
                "headphone_connected": False,
            }},
            "source_tier": "PublicApi",
            "app_package": None,
            "uid": None,
        })
    bp_raw = case.get("behavior_profile", {})
    behavior_profile = {
        "summary": case.get("persona", ""),
        "observation_windows": bp_raw.get("history_len", 1),
    }
    if bp_raw.get("recent_app_categories"):
        behavior_profile["recent_app_categories"] = bp_raw["recent_app_categories"]
    if bp_raw.get("current_app_category"):
        behavior_profile["current_app_category"] = bp_raw["current_app_category"]
    if bp_raw.get("prediction_horizon_secs"):
        behavior_profile["prediction_horizon_secs"] = bp_raw["prediction_horizon_secs"]
    return {
        "current_context": {
            "window_id": case["id"],
            "window_start_ms": ts,
            "window_end_ms": ts + 60_000,
            "duration_secs": 60,
            "events": events,
            "summary": {
                "foreground_apps": case.get("foreground_apps", []),
                "notified_apps": case.get("notified_apps", []),
                "all_semantic_hints": hints,
                "file_activity": case.get("file_activity", []),
                "latest_system_status": latest,
                "source_tier": "PublicApi",
            },
        },
        "behavior_profile": behavior_profile,
        "recent_feedback": [],
    }


def post_chat(endpoint: str, api_key: str, model: str, case: dict[str, Any], timeout: int, temperature: float) -> tuple[dict[str, Any] | None, str | None, int]:
    body = {
        "model": model,
        "temperature": temperature,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": os.environ.get("DIPECS_CLOUD_LLM_SYSTEM_PROMPT", DEFAULT_PROMPT)},
            {"role": "user", "content": "Generate DiPECS intents for this sanitized labeled-evaluation context.\nmodel_input_json=" + json.dumps(build_context(case), ensure_ascii=False)},
        ],
    }
    enable_thinking = os.environ.get("DIPECS_CLOUD_LLM_ENABLE_THINKING")
    if enable_thinking is not None and enable_thinking.strip() != "":
        enabled = enable_thinking.strip().lower() in ("1", "true", "yes", "on")
        body["thinking"] = {"type": "enabled" if enabled else "disabled"}
    data = json.dumps(body).encode("utf-8")
    req = request.Request(endpoint, data=data, method="POST")
    req.add_header("Content-Type", "application/json")
    req.add_header("Accept", "application/json")
    req.add_header("Authorization", f"Bearer {api_key}")
    start = time.time()
    try:
        with request.urlopen(req, timeout=timeout) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except HTTPError as exc:
        try:
            error_body = exc.read().decode("utf-8", errors="replace")
        except Exception:
            error_body = ""
        return None, f"HTTP {exc.code} {exc.reason}: {error_body}", int((time.time() - start) * 1000)
    except URLError as exc:
        return None, f"network error: {exc.reason}", int((time.time() - start) * 1000)
    except Exception as exc:
        return None, str(exc), int((time.time() - start) * 1000)
    try:
        message = payload["choices"][0]["message"]
        content = (message.get("content") or "").strip()
        if not content:
            content = (message.get("reasoning_content") or "").strip()
        cleaned = content.removeprefix("```json").removeprefix("```").removesuffix("```").strip()
        if not cleaned.startswith("{"):
            first = cleaned.find("{")
            last = cleaned.rfind("}")
            if first >= 0 and last > first:
                cleaned = cleaned[first:last + 1]
        return json.loads(cleaned), None, int((time.time() - start) * 1000)
    except Exception as exc:
        safe_payload = dict(payload)
        return None, f"parse response failed: {exc}; payload={safe_payload}", int((time.time() - start) * 1000)


def flattened(model_output: dict[str, Any]) -> list[dict[str, Any]]:
    out = []
    for intent in model_output.get("intents", []):
        actions = intent.get("actions") or [{"action_type": "NoOp", "target": None}]
        for action in actions:
            out.append({
                "intent_type": intent.get("intent_type"),
                "target": intent.get("target") or action.get("target"),
                "extension_category": intent.get("extension_category"),
                "action_type": action.get("action_type"),
            })
    return out


def _normalize_target(target: str | None) -> str | None:
    """Strip Android bridge prefixes for comparison (pkg:com.x -> com.x)."""
    if not target:
        return target
    for prefix in ("pkg:", "url:", "uri:", "work:", "cache:", "own:"):
        if target.startswith(prefix):
            return target[len(prefix):]
    return target


def matches(candidate: dict[str, Any], expected: dict[str, Any], match_mode: str = "full") -> bool:
    if match_mode == "action":
        keys = ("action_type",)
    elif match_mode == "core":
        keys = ("intent_type", "action_type", "extension_category")
    else:
        keys = ("intent_type", "action_type", "target", "extension_category")
    for key in keys:
        if key in expected and expected[key] is not None:
            cand_val = candidate.get(key)
            exp_val = expected[key]
            if key == "target":
                cand_val = _normalize_target(cand_val)
                exp_val = _normalize_target(exp_val)
            if cand_val != exp_val:
                return False
    return True


def hit_rank(candidates: list[dict[str, Any]], expected: list[dict[str, Any]], match_mode: str = "full") -> int | None:
    for idx, candidate in enumerate(candidates, 1):
        if any(matches(candidate, exp, match_mode) for exp in expected):
            return idx
    return None


def pct(num: int, den: int) -> float:
    return round(num / den * 100.0, 3) if den else 0.0


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cases", default="data/evaluation/cloud/cloud-llm-accuracy-cases.json")
    parser.add_argument("--rounds", type=int, default=int(os.environ.get("CLOUD_ACCURACY_ROUNDS", "1")))
    parser.add_argument("--limit-cases", type=int)
    parser.add_argument("--match-mode", choices=["full", "core", "action"], default="full")
    parser.add_argument("--top5-reference", type=float, default=90.0)
    parser.add_argument("--out-dir", default="data/evaluation/cloud")
    args = parser.parse_args()

    load_dotenv(Path(".env"))
    endpoint = os.environ.get("DIPECS_CLOUD_LLM_ENDPOINT") or "https://api.deepseek.com/chat/completions"
    model = os.environ.get("DIPECS_CLOUD_LLM_MODEL") or "deepseek-v4-flash"
    api_key = os.environ.get("DIPECS_CLOUD_LLM_API_KEY") or os.environ.get("DEEPSEEK_API_KEY")
    timeout = int(os.environ.get("DIPECS_CLOUD_LLM_TIMEOUT_SECS", "30"))
    temperature = float(os.environ.get("DIPECS_CLOUD_LLM_TEMPERATURE", "0.1"))
    if not api_key:
        raise SystemExit("missing DIPECS_CLOUD_LLM_API_KEY or DEEPSEEK_API_KEY")

    doc = json.loads(Path(args.cases).read_text(encoding="utf-8"))
    cases = doc["cases"][: args.limit_cases]
    scored = errors = top1 = top3 = top5 = any_hits = 0
    rows = []
    latencies = []
    for case in cases:
        for round_idx in range(args.rounds):
            output, error, latency = post_chat(endpoint, api_key, model, case, timeout, temperature)
            latencies.append(latency)
            if error:
                errors += 1
                print(f"{case['id']} round={round_idx+1} ERROR {latency}ms {error}")
                rows.append({"id": case["id"], "round": round_idx + 1, "error": error, "latency_ms": latency})
                continue
            candidates = flattened(output or {})
            expected = case["expected"]
            if args.match_mode == "action":
                expected = [{"action_type": item.get("action_type")} for item in expected]
            rank = hit_rank(candidates, expected, args.match_mode)
            scored += 1
            if rank is not None:
                any_hits += 1
                if rank <= 1:
                    top1 += 1
                if rank <= 3:
                    top3 += 1
                if rank <= 5:
                    top5 += 1
            print(f"{case['id']} round={round_idx+1} {latency}ms rank={rank} candidates={candidates[:5]}")
            rows.append({"id": case["id"], "round": round_idx + 1, "rank": rank, "latency_ms": latency, "candidates": candidates[:5]})

    latencies_sorted = sorted(latencies)
    report = {
        "schema_version": "dipecs.cloud_accuracy.v2",
        "status": "measured_live_api",
        "cases_file": args.cases,
        "environment": {"provider": "deepseek", "model": model, "rounds": args.rounds, "match_mode": args.match_mode},
        "results": {
            "cases": len(cases),
            "scored_rounds": scored,
            "errors": errors,
            "top1_accuracy_pct": pct(top1, scored),
            "top3_accuracy_pct": pct(top3, scored),
            "top5_accuracy_pct": pct(top5, scored),
            "any_hit_accuracy_pct": pct(any_hits, scored),
            "success_rate_pct": pct(scored, scored + errors),
            "latency_p50_ms": latencies_sorted[len(latencies_sorted)//2] if latencies_sorted else 0,
            "latency_p95_ms": latencies_sorted[min(len(latencies_sorted)-1, int(len(latencies_sorted)*0.95))] if latencies_sorted else 0,
        },
        "reference": {"top5_accuracy_pct": args.top5_reference, "top5_meets_reference": pct(top5, scored) >= args.top5_reference},
        "case_results": rows,
    }
    out = Path(args.out_dir) / f"cloud-accuracy-topk-{int(time.time())}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps(report["results"], indent=2))
    print(f"top5 reference {args.top5_reference}% met: {report['reference']['top5_meets_reference']}")
    print(f"Wrote {out}")


if __name__ == "__main__":
    main()
