#!/usr/bin/env python3
"""llama.cpp 参数优化实验脚本。

测试不同配置参数对推理性能的影响：
- --threads: 4, 8, 12
- --batch-size: 32, 64, 128
- --ctx-size: 512, 1024, 2048
- --no-mmap vs 默认 mmap
"""

import json
import subprocess
from pathlib import Path
from typing import Any


LAB4_ROOT = Path(__file__).resolve().parent.parent
MODEL_PATH = LAB4_ROOT / "data" / "models" / "qwen3.5-2b-q4_k_m.gguf"
LLAMA_BENCH = LAB4_ROOT / "third_party" / "llama.cpp" / "build" / "bin" / "llama-bench"
OUTPUT_DIR = LAB4_ROOT / "data" / "results" / "param-opt"


def build_bench_command(
    *,
    bench_path: Path,
    model_path: Path,
    threads: int,
    batch_size: int,
    n_prompt: int,
    n_gen: int,
    repetitions: int,
    use_mmap: bool,
) -> list[str]:
    """构造只包含一组参数的 llama-bench 命令。"""
    command = [
        str(bench_path),
        "--model",
        str(model_path),
        "--threads",
        str(threads),
        "--batch-size",
        str(batch_size),
        "--n-prompt",
        str(n_prompt),
        "--n-gen",
        str(n_gen),
        "--repetitions",
        str(repetitions),
        "--n-gpu-layers",
        "0",
        "--output",
        "jsonl",
    ]
    if not use_mmap:
        command.extend(["--mmap", "0"])
    return command


def select_records(
    records: list[dict[str, Any]],
    *,
    threads: int,
    batch_size: int,
    n_prompt: int,
    n_gen: int,
    use_mmap: bool | None = None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    """从 llama-bench 输出中选择与目标配置完全匹配的两条记录。"""

    def matches(record: dict[str, Any], prompt_tokens: int, generation_tokens: int) -> bool:
        mmap_matches = use_mmap is None or record.get("use_mmap") is use_mmap
        return (
            record.get("n_threads") == threads
            and record.get("n_batch") == batch_size
            and record.get("n_prompt") == prompt_tokens
            and record.get("n_gen") == generation_tokens
            and mmap_matches
        )

    prompt_record = next(
        (record for record in records if matches(record, n_prompt, 0)),
        None,
    )
    generation_record = next(
        (record for record in records if matches(record, 0, n_gen)),
        None,
    )
    if prompt_record is None or generation_record is None:
        raise ValueError(
            "llama-bench output does not contain the requested configuration: "
            f"threads={threads}, batch_size={batch_size}, n_prompt={n_prompt}, "
            f"n_gen={n_gen}, use_mmap={use_mmap}"
        )
    return prompt_record, generation_record


def run_bench(
    name: str,
    *,
    threads: int = 12,
    batch_size: int = 64,
    n_prompt: int = 128,
    n_gen: int = 64,
    repetitions: int = 3,
    use_mmap: bool = True,
) -> dict[str, Any]:
    """运行单一配置的 llama-bench 并解析结果。"""
    command = build_bench_command(
        bench_path=LLAMA_BENCH,
        model_path=MODEL_PATH,
        threads=threads,
        batch_size=batch_size,
        n_prompt=n_prompt,
        n_gen=n_gen,
        repetitions=repetitions,
        use_mmap=use_mmap,
    )

    print(f"\n>>> Running: {name}")
    print(f"    command: {' '.join(command)}")

    result = subprocess.run(command, capture_output=True, text=True)

    if result.returncode != 0:
        print(f"    ERROR: {result.stderr[:200]}")
        return {"name": name, "error": result.stderr}

    lines = result.stdout.strip().split("\n")
    records = []
    for line in lines:
        if line.strip():
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError:
                pass

    try:
        prompt_record, generation_record = select_records(
            records,
            threads=threads,
            batch_size=batch_size,
            n_prompt=n_prompt,
            n_gen=n_gen,
            use_mmap=use_mmap,
        )
    except ValueError as error:
        return {"name": name, "error": str(error), "raw_records": records}

    return {
        "name": name,
        "config": {
            "threads": threads,
            "batch_size": batch_size,
            "n_prompt": n_prompt,
            "n_gen": n_gen,
            "repetitions": repetitions,
            "use_mmap": use_mmap,
        },
        "command": command,
        "prompt_tps": prompt_record["avg_ts"],
        "gen_tps": generation_record["avg_ts"],
        "raw_records": records,
    }


def main() -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    results = []

    # 1. threads 对比（固定 batch=64）
    for threads in [4, 8, 12]:
        res = run_bench(f"threads-{threads}", threads=threads)
        results.append(res)

    # 2. batch-size 对比（固定 threads=12）
    for batch in [32, 64, 128]:
        res = run_bench(
            f"batch-{batch}",
            batch_size=batch,
        )
        results.append(res)

    # 3. n-prompt 对比（模拟不同 ctx-size 的输入长度，固定 threads=12, batch=64）
    for np in [128, 512, 1024]:
        res = run_bench(
            f"n-prompt-{np}",
            n_prompt=np,
        )
        results.append(res)

    # 4. mmap 对比（固定 threads=12, batch=64）
    res_mmap = run_bench("mmap-default")
    results.append(res_mmap)

    res_no_mmap = run_bench("no-mmap", use_mmap=False)
    results.append(res_no_mmap)

    # 保存详细结果
    detail_path = OUTPUT_DIR / "param-opt-detail.jsonl"
    with open(detail_path, "w", encoding="utf-8") as f:
        for r in results:
            f.write(json.dumps(r, default=str, ensure_ascii=False) + "\n")

    # 保存汇总表格
    summary = []
    for r in results:
        if "error" not in r:
            summary.append(
                {
                    "配置": r["name"],
                    "Prompt (t/s)": round(r.get("prompt_tps", 0), 2),
                    "Generation (t/s)": round(r.get("gen_tps", 0), 2),
                }
            )

    summary_path = OUTPUT_DIR / "param-opt-summary.json"
    with open(summary_path, "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=2, ensure_ascii=False)

    # 打印汇总
    print("\n" + "=" * 60)
    print("参数优化实验结果汇总")
    print("=" * 60)
    print(f"{'配置':<20} {'Prompt (t/s)':>15} {'Generation (t/s)':>18}")
    print("-" * 60)
    for s in summary:
        print(f"{s['配置']:<20} {s['Prompt (t/s)']:>15.2f} {s['Generation (t/s)']:>18.2f}")
    print("=" * 60)
    print(f"\n详细结果: {detail_path}")
    print(f"汇总表格: {summary_path}")


if __name__ == "__main__":
    main()
