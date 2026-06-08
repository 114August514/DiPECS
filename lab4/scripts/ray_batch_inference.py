#!/usr/bin/env python3
"""Ray 批量推理实验脚本。

使用单机多进程模拟多机环境：
- 2 个 llama-server 实例（端口 8080、8081），模拟两台异构机器
- 1 个 Ray head + 2 个 Ray worker，每个 worker 绑定到一个 server
- 测试：串行、单机固定分配、轮询调度、基于历史延迟调度
"""

import argparse
import json
import math
import statistics
import time
from pathlib import Path
from collections.abc import Callable
from typing import Any

import ray
import requests


SERVERS = {
    "s1": "http://127.0.0.1:8080",   # 模拟较快机器：threads=8
    "s2": "http://127.0.0.1:8081",   # 模拟较慢机器：threads=4
}

PROMPTS_PATH = Path(__file__).parent.parent / "data" / "prompts" / "batch-prompts.jsonl"
OUTPUT_PATH = Path(__file__).parent.parent / "data" / "results" / "ray-batch-results.jsonl"
BASE_PROMPT_COUNT = 20
STRATEGY_NAMES = (
    "serial",
    "fixed_partition",
    "round_robin",
    "latency_based",
)


def parse_arguments(arguments: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--prompt-count",
        type=int,
        default=BASE_PROMPT_COUNT,
        help="从 prompt 数据集开头选取的请求数量",
    )
    parser.add_argument(
        "--strategies",
        nargs="+",
        choices=STRATEGY_NAMES,
        default=list(STRATEGY_NAMES),
        help="需要执行的调度策略",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=OUTPUT_PATH,
        help="详细 JSONL 结果路径",
    )
    parsed = parser.parse_args(arguments)
    if parsed.prompt_count <= 0:
        parser.error("--prompt-count must be greater than zero")
    return parsed


def ray_init_options() -> dict[str, str]:
    """单机模拟直接复用当前环境，避免重复打包模型和依赖。"""
    return {"address": "auto"}


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


def call_server(server_url: str, prompt: str, max_tokens: int) -> dict[str, Any]:
    """调用 llama-server 的 /completion API。"""
    start = time.perf_counter()
    try:
        resp = requests.post(
            f"{server_url}/completion",
            json={
                "prompt": prompt,
                "n_predict": max_tokens,
                "temperature": 0.7,
                "seed": 42,
                "stream": False,
            },
            timeout=180,
        )
        resp.raise_for_status()
        data = resp.json()
        elapsed_ms = (time.perf_counter() - start) * 1000
        timings = data.get("timings", {})
        return {
            "success": True,
            "server_url": server_url,
            "content_len": len(data.get("content", "")),
            "tokens_predicted": data.get("tokens_predicted", 0),
            "predicted_per_second": timings.get("predicted_per_second", 0.0),
            "predicted_ms": timings.get("predicted_ms", 0.0),
            "prompt_ms": timings.get("prompt_ms", 0.0),
            "total_ms": elapsed_ms,
        }
    except Exception as exc:
        elapsed_ms = (time.perf_counter() - start) * 1000
        return {
            "success": False,
            "server_url": server_url,
            "error": str(exc),
            "total_ms": elapsed_ms,
        }


# Ray Task：绑定到 server_s1 资源
@ray.remote(resources={"server_s1": 0.6})
def infer_s1(prompt: str, max_tokens: int) -> dict[str, Any]:
    return call_server(SERVERS["s1"], prompt, max_tokens)


# Ray Task：绑定到 server_s2 资源
@ray.remote(resources={"server_s2": 0.6})
def infer_s2(prompt: str, max_tokens: int) -> dict[str, Any]:
    return call_server(SERVERS["s2"], prompt, max_tokens)


TASK_MAP = {"s1": infer_s1, "s2": infer_s2}


def collect_remote_results(
    submit: Callable[[], list[Any]],
) -> tuple[list[dict[str, Any]], float]:
    """从任务提交前开始计时，覆盖 Ray 调度和结果收集。"""
    start = time.perf_counter()
    refs = submit()
    results = ray.get(refs)
    elapsed_ms = (time.perf_counter() - start) * 1000
    return results, elapsed_ms


def run_serial(
    prompts: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], float]:
    """串行调用 s1。"""
    results = []
    start = time.perf_counter()
    for p in prompts:
        res = call_server(SERVERS["s1"], p["prompt"], p["max_tokens"])
        res["prompt_id"] = p["id"]
        res["strategy"] = "serial"
        res["server"] = "s1"
        results.append(res)
    total = (time.perf_counter() - start) * 1000
    return results, total


def run_fixed_partition(
    prompts: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], float]:
    """固定分配：前一半给 s1，后一半给 s2。"""
    mid = len(prompts) // 2

    def submit() -> list[Any]:
        refs = [
            infer_s1.remote(prompt["prompt"], prompt["max_tokens"])
            for prompt in prompts[:mid]
        ]
        refs.extend(
            infer_s2.remote(prompt["prompt"], prompt["max_tokens"])
            for prompt in prompts[mid:]
        )
        return refs

    raw_results, total = collect_remote_results(submit)

    results = []
    for index, (p, r) in enumerate(zip(prompts, raw_results)):
        r["prompt_id"] = p["id"]
        r["strategy"] = "fixed_partition"
        r["server"] = "s1" if index < mid else "s2"
        results.append(r)
    return results, total


def run_round_robin(
    prompts: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], float]:
    """轮询调度。"""
    servers = ["s1", "s2"]
    mapping = [servers[index % len(servers)] for index in range(len(prompts))]

    def submit() -> list[Any]:
        refs = []
        for prompt, server in zip(prompts, mapping):
            refs.append(
                TASK_MAP[server].remote(prompt["prompt"], prompt["max_tokens"])
            )
        return refs

    raw_results, total = collect_remote_results(submit)

    results = []
    for p, r, srv in zip(prompts, raw_results, mapping):
        r["prompt_id"] = p["id"]
        r["strategy"] = "round_robin"
        r["server"] = srv
        results.append(r)
    return results, total


def latency_based_mapping(
    *,
    prompt_count: int,
    s1_latency_ms: float,
    s2_latency_ms: float,
) -> list[str]:
    """按延迟倒数权重生成确定性的节点分配。"""
    if prompt_count < 2:
        return ["s1"] * prompt_count
    total_latency = s1_latency_ms + s2_latency_ms
    weight_s1 = s2_latency_ms / total_latency
    target_s1 = round(prompt_count * weight_s1)
    target_s1 = max(1, min(prompt_count - 1, target_s1))
    return ["s1"] * target_s1 + ["s2"] * (prompt_count - target_s1)


def run_latency_based(
    prompts: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], float]:
    """基于历史平均延迟的调度：先 warm-up，然后按延迟比例分配。"""
    start = time.perf_counter()

    # warm-up：每个 server 跑一个 prompt 获取基准延迟
    warm_prompt = prompts[0]
    s1_res = ray.get(infer_s1.remote(warm_prompt["prompt"], warm_prompt["max_tokens"]))
    s2_res = ray.get(infer_s2.remote(warm_prompt["prompt"], warm_prompt["max_tokens"]))

    s1_latency = s1_res.get("total_ms", 1)
    s2_latency = s2_res.get("total_ms", 1)

    # 延迟越低，分配越多：按反比例权重。
    total_latency = s1_latency + s2_latency
    weight_s1 = s2_latency / total_latency
    weight_s2 = s1_latency / total_latency

    mapping = latency_based_mapping(
        prompt_count=len(prompts),
        s1_latency_ms=s1_latency,
        s2_latency_ms=s2_latency,
    )

    refs = [
        TASK_MAP[server].remote(prompt["prompt"], prompt["max_tokens"])
        for prompt, server in zip(prompts, mapping)
    ]
    raw_results = ray.get(refs)
    elapsed = (time.perf_counter() - start) * 1000

    results = []
    for p, r, srv in zip(prompts, raw_results, mapping):
        r["prompt_id"] = p["id"]
        r["strategy"] = "latency_based"
        r["server"] = srv
        results.append(r)

    # 在结果中附加 warm-up 延迟信息
    results[0]["warmup_s1_latency_ms"] = s1_latency
    results[0]["warmup_s2_latency_ms"] = s2_latency
    results[0]["weight_s1"] = weight_s1
    results[0]["weight_s2"] = weight_s2
    return results, elapsed


def summarize(
    results: list[dict[str, Any]],
    strategy: str,
    wall_ms: float,
) -> dict[str, Any]:
    ok = [r for r in results if r.get("success")]
    fail = [r for r in results if not r.get("success")]
    per_server: dict[str, list[float]] = {}
    for r in ok:
        per_server.setdefault(r.get("server", "unknown"), []).append(r.get("total_ms", 0))

    server_stats = {}
    for srv, vals in per_server.items():
        server_stats[srv] = {
            "count": len(vals),
            "avg_ms": statistics.mean(vals),
            "median_ms": statistics.median(vals),
            "p95_ms": nearest_rank_percentile(vals, 0.95),
            "min_ms": min(vals),
            "max_ms": max(vals),
        }

    tokens_total = sum(r.get("tokens_predicted", 0) for r in ok)
    return {
        "strategy": strategy,
        "total_prompts": len(results),
        "success": len(ok),
        "failed": len(fail),
        "wall_time_ms": wall_ms,
        "throughput_prompts_per_sec": len(ok) / (wall_ms / 1000),
        "throughput_tokens_per_sec": tokens_total / (wall_ms / 1000),
        "avg_latency_ms": statistics.mean([r["total_ms"] for r in ok]) if ok else 0,
        "server_stats": server_stats,
    }


def main() -> None:
    arguments = parse_arguments()
    ray.init(**ray_init_options())
    print("Connected to Ray cluster:", ray.cluster_resources())

    prompts = load_prompts(PROMPTS_PATH, limit=arguments.prompt_count)
    if len(prompts) < arguments.prompt_count:
        raise ValueError(
            f"requested {arguments.prompt_count} prompts, but only "
            f"{len(prompts)} are available"
        )
    print(f"Loaded {len(prompts)} prompts")

    arguments.output.parent.mkdir(parents=True, exist_ok=True)

    all_results = []
    summaries = []

    runners = {
        "serial": run_serial,
        "fixed_partition": run_fixed_partition,
        "round_robin": run_round_robin,
        "latency_based": run_latency_based,
    }
    strategy_count = len(arguments.strategies)
    for index, strategy in enumerate(arguments.strategies, start=1):
        print(f"\n[{index}/{strategy_count}] Running {strategy}...")
        results, wall_ms = runners[strategy](prompts)
        all_results.extend(results)
        summaries.append(summarize(results, strategy, wall_ms))

    # 保存详细结果
    with open(arguments.output, "w", encoding="utf-8") as f:
        for r in all_results:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")

    # 保存汇总
    summary_path = arguments.output.with_suffix(".summary.json")
    with open(summary_path, "w", encoding="utf-8") as f:
        json.dump(summaries, f, indent=2, ensure_ascii=False)

    print(f"\nResults saved to {arguments.output}")
    print(f"Summary saved to {summary_path}")

    print("\n=== Summary ===")
    for s in summaries:
        print(f"\n{s['strategy']}")
        print(f"  wall time: {s['wall_time_ms']:.1f} ms")
        print(f"  success: {s['success']}/{s['total_prompts']}")
        print(f"  throughput: {s['throughput_prompts_per_sec']:.2f} prompts/s, {s['throughput_tokens_per_sec']:.2f} tokens/s")
        print(f"  avg latency: {s['avg_latency_ms']:.1f} ms")
        for srv, st in s["server_stats"].items():
            print(f"  server {srv}: count={st['count']}, avg={st['avg_ms']:.1f}ms, p95={st['p95_ms']:.1f}ms")


if __name__ == "__main__":
    main()
