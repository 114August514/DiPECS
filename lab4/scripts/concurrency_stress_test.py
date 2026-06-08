#!/usr/bin/env python3
"""并发压力测试脚本。

对 llama-server 设置 3 档并发度（1, 2, 4），使用同一组 20 个 prompt，
记录平均延迟、P95 延迟、吞吐量和失败请求数。
"""

import asyncio
import json
import math
import statistics
import time
from pathlib import Path
from typing import Any

import aiohttp


PROMPTS_PATH = Path(__file__).parent.parent / "data" / "prompts" / "batch-prompts.jsonl"
SERVER_URL = "http://127.0.0.1:8080"
OUTPUT_DIR = Path(__file__).parent.parent / "data" / "results" / "concurrency-stress"
STRESS_PROMPT_COUNT = 20


def load_prompts(path: Path, *, limit: int | None = None) -> list[dict[str, Any]]:
    prompts = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                prompts.append(json.loads(line))
    return prompts[:limit]


def nearest_rank_percentile(values: list[float], percentile: float) -> float:
    if not values:
        raise ValueError("percentile requires at least one value")
    if not 0 < percentile <= 1:
        raise ValueError("percentile must be in the range (0, 1]")
    ordered = sorted(values)
    index = math.ceil(len(ordered) * percentile) - 1
    return ordered[index]


async def infer_one(
    session: aiohttp.ClientSession,
    prompt: str,
    max_tokens: int,
    prompt_id: str,
) -> dict[str, Any]:
    start = time.perf_counter()
    try:
        async with session.post(
            f"{SERVER_URL}/completion",
            json={
                "prompt": prompt,
                "n_predict": max_tokens,
                "temperature": 0.7,
                "seed": 42,
                "stream": False,
            },
            timeout=aiohttp.ClientTimeout(total=120),
        ) as resp:
            data = await resp.json()
            elapsed_ms = (time.perf_counter() - start) * 1000
            timings = data.get("timings", {})
            return {
                "success": True,
                "prompt_id": prompt_id,
                "total_ms": elapsed_ms,
                "predicted_per_second": timings.get("predicted_per_second", 0.0),
                "predicted_ms": timings.get("predicted_ms", 0.0),
                "tokens_predicted": data.get("tokens_predicted", 0),
            }
    except Exception as exc:
        elapsed_ms = (time.perf_counter() - start) * 1000
        return {
            "success": False,
            "prompt_id": prompt_id,
            "total_ms": elapsed_ms,
            "error": str(exc),
        }


async def run_with_concurrency(
    prompts: list[dict[str, Any]],
    concurrency: int,
) -> list[dict[str, Any]]:
    """以指定并发度运行所有 prompt。"""
    semaphore = asyncio.Semaphore(concurrency)

    async def bounded_infer(session, p):
        async with semaphore:
            return await infer_one(session, p["prompt"], p["max_tokens"], p["id"])

    async with aiohttp.ClientSession() as session:
        tasks = [bounded_infer(session, p) for p in prompts]
        return await asyncio.gather(*tasks)


def summarize(
    results: list[dict[str, Any]],
    concurrency: int,
    wall_ms: float,
) -> dict[str, Any]:
    ok = [r for r in results if r.get("success")]
    fail = [r for r in results if not r.get("success")]

    if ok:
        latencies = [r["total_ms"] for r in ok]
        p95 = nearest_rank_percentile(latencies, 0.95)
        tokens_total = sum(r.get("tokens_predicted", 0) for r in ok)

        return {
            "concurrency": concurrency,
            "total_prompts": len(results),
            "success": len(ok),
            "failed": len(fail),
            "wall_time_ms": wall_ms,
            "throughput_prompts_per_sec": len(ok) / (wall_ms / 1000),
            "throughput_tokens_per_sec": tokens_total / (wall_ms / 1000),
            "avg_latency_ms": statistics.mean(latencies),
            "median_latency_ms": statistics.median(latencies),
            "p95_latency_ms": p95,
            "min_latency_ms": min(latencies),
            "max_latency_ms": max(latencies),
            "stddev_latency_ms": statistics.stdev(latencies) if len(latencies) > 1 else 0,
        }
    else:
        return {
            "concurrency": concurrency,
            "total_prompts": len(results),
            "success": 0,
            "failed": len(fail),
            "wall_time_ms": wall_ms,
        }


async def main():
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    prompts = load_prompts(PROMPTS_PATH, limit=STRESS_PROMPT_COUNT)
    print(f"Loaded {len(prompts)} prompts")

    all_results = []
    summaries = []

    for concurrency in [1, 2, 4]:
        print(f"\n>>> Testing concurrency={concurrency}")
        start = time.perf_counter()
        results = await run_with_concurrency(prompts, concurrency)
        wall_ms = (time.perf_counter() - start) * 1000

        for r in results:
            r["concurrency"] = concurrency
        all_results.extend(results)

        summary = summarize(results, concurrency, wall_ms)
        summaries.append(summary)

        print(f"  wall time: {wall_ms:.1f} ms")
        print(f"  success: {summary['success']}/{summary['total_prompts']}")
        print(f"  throughput: {summary['throughput_prompts_per_sec']:.2f} prompts/s")
        if summary['success'] > 0:
            print(f"  avg latency: {summary['avg_latency_ms']:.1f} ms")
            print(f"  p95 latency: {summary['p95_latency_ms']:.1f} ms")

    # 保存
    detail_path = OUTPUT_DIR / "concurrency-stress-detail.jsonl"
    with open(detail_path, "w", encoding="utf-8") as f:
        for r in all_results:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")

    summary_path = OUTPUT_DIR / "concurrency-stress-summary.json"
    with open(summary_path, "w", encoding="utf-8") as f:
        json.dump(summaries, f, indent=2, ensure_ascii=False)

    # 打印最终表格
    print("\n" + "=" * 80)
    print("并发压力测试汇总")
    print("=" * 80)
    print(f"{'并发度':>8} {'总耗时(s)':>12} {'成功/总数':>12} {'吞吐(p/s)':>12} {'平均(ms)':>12} {'P95(ms)':>12} {'失败数':>8}")
    print("-" * 80)
    for s in summaries:
        print(
            f"{s['concurrency']:>8} "
            f"{s['wall_time_ms']/1000:>12.2f} "
            f"{s['success']}/{s['total_prompts']:>8} "
            f"{s['throughput_prompts_per_sec']:>12.2f} "
            f"{s.get('avg_latency_ms', 0):>12.1f} "
            f"{s.get('p95_latency_ms', 0):>12.1f} "
            f"{s['failed']:>8}"
        )
    print("=" * 80)
    print(f"\n详细结果: {detail_path}")
    print(f"汇总表格: {summary_path}")


if __name__ == "__main__":
    asyncio.run(main())
