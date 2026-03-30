"""
benchmark.py
------------
Phase 1 spike benchmark. Measures proving time, peak RAM, and proof size.
Run this after setup_circuit.py to validate your environment.

Usage:
    python3 benchmark.py [--artifacts-dir ./artifacts] [--runs 3]

Screenshot the output — this is your Phase 1 deliverable.
"""

import argparse
import asyncio
import json
import os
import statistics
import sys
import time
import tracemalloc
import ezkl


async def run_single_proof(artifacts_dir: str) -> tuple[float, int, float]:
    compiled_path = os.path.join(artifacts_dir, "model.compiled")
    pk_path       = os.path.join(artifacts_dir, "pk.key")
    vk_path       = os.path.join(artifacts_dir, "vk.key")
    witness_path  = os.path.join(artifacts_dir, "witness.json")
    proof_path    = os.path.join(artifacts_dir, "proof.json")
    input_path    = os.path.join(artifacts_dir, "input.json")

    ezkl.gen_witness(input_path, compiled_path, witness_path)

    tracemalloc.start()
    start = time.perf_counter()
    res = ezkl.prove(witness_path, compiled_path, pk_path, proof_path)
    elapsed = time.perf_counter() - start
    _, peak_ram_bytes = tracemalloc.get_traced_memory()
    tracemalloc.stop()

    if not res:
        print("[benchmark] Proof failed", file=sys.stderr)
        sys.exit(1)

    proof_size_kb = os.path.getsize(proof_path) / 1024
    return elapsed, peak_ram_bytes, proof_size_kb


async def main() -> None:
    parser = argparse.ArgumentParser(description="Phase 1 EZKL benchmark")
    parser.add_argument("--artifacts-dir", default="./artifacts")
    parser.add_argument("--runs", type=int, default=3)
    args = parser.parse_args()

    artifacts_dir = args.artifacts_dir

    for required in ["model.compiled", "pk.key", "vk.key", "input.json"]:
        path = os.path.join(artifacts_dir, required)
        if not os.path.exists(path):
            print(f"[benchmark] Missing: {path}. Run setup_circuit.py first.")
            sys.exit(1)

    print(f"\n[benchmark] Running {args.runs} proof(s)...\n")

    times, rams, sizes = [], [], []

    for i in range(args.runs):
        print(f"  Run {i + 1}/{args.runs}...", end=" ", flush=True)
        elapsed, peak_ram, proof_size = await run_single_proof(artifacts_dir)
        times.append(elapsed)
        rams.append(peak_ram)
        sizes.append(proof_size)
        print(f"{elapsed:.1f}s")

    avg_time = statistics.mean(times)
    avg_ram  = statistics.mean(rams)
    avg_size = statistics.mean(sizes)

    time_ok = "✓" if avg_time < 30 else "✗"
    ram_ok  = "✓" if avg_ram / 1e9 < 50 else "✗"
    size_ok = "✓" if avg_size < 5 else "✗"

    print(f"""
╔══════════════════════════════════════════╗
║         Phase 1 Benchmark Results        ║
╠══════════════════════════════════════════╣
║  Runs          : {args.runs:<25}║
╠══════════════════════════════════════════╣
║  Proving time  : {avg_time:<6.1f}s   target <30s  {time_ok}  ║
║  Peak RAM      : {avg_ram / 1e9:<6.1f}GB  target <50GB {ram_ok}  ║
║  Proof size    : {avg_size:<6.1f}KB  target <5KB  {size_ok}  ║
╚══════════════════════════════════════════╝
""")

    results = {
        "runs": args.runs,
        "avg_proving_time_s": round(avg_time, 2),
        "avg_peak_ram_gb": round(avg_ram / 1e9, 2),
        "avg_proof_size_kb": round(avg_size, 2),
        "targets_met": {
            "proving_time": avg_time < 30,
            "peak_ram": avg_ram / 1e9 < 50,
            "proof_size": avg_size < 5,
        },
    }

    results_path = os.path.join(artifacts_dir, "benchmark_results.json")
    with open(results_path, "w") as f:
        json.dump(results, f, indent=2)
    print(f"[benchmark] Results saved to {results_path}")


if __name__ == "__main__":
    asyncio.run(main())
