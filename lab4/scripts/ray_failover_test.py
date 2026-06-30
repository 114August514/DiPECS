#!/usr/bin/env python3
"""Ray 失败重试实验脚本。

实验流程：
1. 并发提交 20 个 prompt（轮询分配到 s1/s2）
2. 提交后立即 kill server s2（制造故障）
3. 等待所有 Task 完成，s2 的 Task 会失败
4. 收集失败的 prompt，重新提交到 s1
5. 记录重试日志和最终成功率
"""

import json
import subprocess
import time
from pathlib import Path
from typing import List, Dict

import ray
import requests


SERVERS = {
    "s1": "http://127.0.0.1:8080",
    "s2": "http://127.0.0.1:8081",
}

PROMPTS_PATH = Path(__file__).parent.parent / "data" / "prompts" / "batch-prompts.jsonl"
OUTPUT_DIR = Path(__file__).parent.parent / "data" / "results" / "ray-failover"


def load_prompts(path: Path) -> List[Dict]:
    prompts = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                prompts.append(json.loads(line))
    return prompts


def call_server(server_url: str, prompt: str, max_tokens: int) -> Dict:
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
            timeout=60,
        )
        resp.raise_for_status()
        data = resp.json()
        elapsed_ms = (time.perf_counter() - start) * 1000
        timings = data.get("timings", {})
        return {
            "success": True,
            "server_url": server_url,
            "total_ms": elapsed_ms,
            "predicted_per_second": timings.get("predicted_per_second", 0.0),
            "tokens_predicted": data.get("tokens_predicted", 0),
        }
    except Exception as exc:
        elapsed_ms = (time.perf_counter() - start) * 1000
        return {
            "success": False,
            "server_url": server_url,
            "error": str(exc),
            "total_ms": elapsed_ms,
        }


@ray.remote(resources={"server_s1": 0.5})
def infer_s1(prompt: str, max_tokens: int) -> Dict:
    return call_server(SERVERS["s1"], prompt, max_tokens)


@ray.remote(resources={"server_s2": 0.5})
def infer_s2(prompt: str, max_tokens: int) -> Dict:
    return call_server(SERVERS["s2"], prompt, max_tokens)


def kill_server_s2():
    """手动注入故障：kill 掉 s2 的 llama-server。"""
    print("\n[FAILOVER] Injecting failure: killing server s2 (port 8081)...")
    result = subprocess.run(
        ["pkill", "-f", "llama-server.*port 8081"],
        capture_output=True,
        text=True,
    )
    time.sleep(2)
    print(f"[FAILOVER] pkill result: {result.returncode}")


def main():
    ray.init(address="auto")
    print("Connected to Ray cluster:", ray.cluster_resources())

    prompts = load_prompts(PROMPTS_PATH)
    # 取前 20 个做故障注入实验（节省时间）
    prompts = prompts[:20]
    print(f"Loaded {len(prompts)} prompts for failover test")

    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    # Phase 1: 并发提交所有 prompt（轮询分配）
    print("\n[Phase 1] Submitting all prompts concurrently (round-robin)...")
    refs = []
    mapping = []
    for i, p in enumerate(prompts):
        srv = "s1" if i % 2 == 0 else "s2"
        if srv == "s1":
            refs.append(infer_s1.remote(p["prompt"], p["max_tokens"]))
        else:
            refs.append(infer_s2.remote(p["prompt"], p["max_tokens"]))
        mapping.append(srv)

    # 提交后立即注入故障（kill s2）
    kill_server_s2()

    # 等待所有 Task 完成
    print("[Phase 1] Waiting for all tasks to complete...")
    raw_results = ray.get(refs)

    phase1_results = []
    failed_prompts = []
    for i, (p, r, srv) in enumerate(zip(prompts, raw_results, mapping)):
        r["prompt_id"] = p["id"]
        r["phase"] = 1
        r["server"] = srv
        phase1_results.append(r)
        if not r.get("success"):
            failed_prompts.append((i, p, srv))

    success_count_p1 = sum(1 for r in phase1_results if r.get("success"))
    fail_count_p1 = len(failed_prompts)
    print(f"[Phase 1] Success: {success_count_p1}/{len(prompts)}, Failed: {fail_count_p1}")
    for idx, p, srv in failed_prompts:
        print(f"  - FAILED: {p['id']} (assigned to {srv})")

    # Phase 2: 将失败的 prompt 重试到 s1
    print(f"\n[Phase 2] Retrying {len(failed_prompts)} failed prompts on server s1...")
    retry_refs = []
    for idx, p, _ in failed_prompts:
        retry_refs.append(infer_s1.remote(p["prompt"], p["max_tokens"]))

    retry_results = ray.get(retry_refs) if retry_refs else []

    phase2_results = []
    for (idx, p, orig_srv), r in zip(failed_prompts, retry_results):
        r["prompt_id"] = p["id"]
        r["phase"] = 2
        r["server"] = "s1"
        r["orig_server"] = orig_srv
        r["is_retry"] = True
        phase2_results.append(r)

    success_count_p2 = sum(1 for r in phase2_results if r.get("success"))
    print(f"[Phase 2] Retry success: {success_count_p2}/{len(failed_prompts)}")

    # 汇总
    final_success = success_count_p1 + success_count_p2
    final_total = len(prompts)
    final_rate = final_success / final_total * 100

    print("\n" + "=" * 60)
    print("失败重试实验汇总")
    print("=" * 60)
    print(f"总请求数:        {final_total}")
    print(f"Phase 1 成功:    {success_count_p1}")
    print(f"Phase 1 失败:    {fail_count_p1}")
    print(f"Phase 2 重试成功:{success_count_p2}")
    print(f"Phase 2 重试失败:{fail_count_p1 - success_count_p2}")
    print(f"最终成功率:      {final_success}/{final_total} = {final_rate:.1f}%")
    print("=" * 60)

    # 保存结果
    all_results = phase1_results + phase2_results
    detail_path = OUTPUT_DIR / "ray-failover-detail.jsonl"
    with open(detail_path, "w", encoding="utf-8") as f:
        for r in all_results:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")

    summary = {
        "total": final_total,
        "phase1_success": success_count_p1,
        "phase1_failed": fail_count_p1,
        "phase2_retry_success": success_count_p2,
        "phase2_retry_failed": fail_count_p1 - success_count_p2,
        "final_success": final_success,
        "final_success_rate": final_rate,
        "failure_injection": "killed llama-server on port 8081 (server s2)",
    }
    summary_path = OUTPUT_DIR / "ray-failover-summary.json"
    with open(summary_path, "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=2, ensure_ascii=False)

    print(f"\n详细结果: {detail_path}")
    print(f"汇总表格: {summary_path}")


if __name__ == "__main__":
    main()
