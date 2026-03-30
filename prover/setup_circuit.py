"""
setup_circuit.py
----------------
Compiles model.onnx into an arithmetic circuit and generates
proving + verification keys.

Compatible with ezkl 23.x on Python 3.9 (Mac ARM).

Usage:
    python setup_circuit.py [--artifacts-dir ./artifacts]
"""

import argparse
import asyncio
import os
import sys

import ezkl


def check(result, step: str) -> None:
    print(f"[setup] {step} — {'done' if result else 'FAILED'}")
    if not result:
        sys.exit(1)


async def run_setup(artifacts_dir: str) -> None:
    model_path    = os.path.join(artifacts_dir, "model.onnx")
    settings_path = os.path.join(artifacts_dir, "settings.json")
    compiled_path = os.path.join(artifacts_dir, "model.compiled")
    vk_path       = os.path.join(artifacts_dir, "vk.key")
    pk_path       = os.path.join(artifacts_dir, "pk.key")

    if not os.path.exists(model_path):
        print(f"[setup] ERROR: model.onnx not found at {model_path}")
        print(f"[setup] Run export_model.py first.")
        sys.exit(1)

    # Step 1 — gen settings (sync)
    print("[setup] Step 1/4 — Generating circuit settings...")
    try:
        res = ezkl.gen_settings(model_path, settings_path)
        check(res, "gen_settings")
    except Exception as e:
        print(f"[setup] gen_settings raised: {e}")
        sys.exit(1)

    # Step 2 — compile circuit (sync)
    print("[setup] Step 2/4 — Compiling circuit...")
    try:
        res = ezkl.compile_circuit(model_path, compiled_path, settings_path)
        check(res, "compile_circuit")
    except Exception as e:
        print(f"[setup] compile_circuit raised: {e}")
        sys.exit(1)

    # Step 3 — fetch SRS (async in ezkl 23.x)
    print("[setup] Step 3/4 — Fetching SRS (may take several minutes)...")
    try:
        res = ezkl.get_srs(settings_path)
        check(res, "get_srs")
    except Exception as e:
        print(f"[setup] get_srs raised: {e}")
        sys.exit(1)

    # Step 4 — generate pk/vk (sync, slow, RAM intensive)
    print("[setup] Step 4/4 — Generating keys (slow — watch RAM)...")
    try:
        res = ezkl.setup(compiled_path, vk_path, pk_path)
        check(res, "setup")
    except Exception as e:
        print(f"[setup] setup raised: {e}")
        sys.exit(1)

    print(f"\n[setup] All done. Artifacts in: {artifacts_dir}/")
    print(f"  model.compiled : {compiled_path}")
    print(f"  vk.key         : {vk_path}")
    print(f"  pk.key         : {pk_path}")
    print(f"\n[setup] Sizes:")
    for p in [compiled_path, vk_path, pk_path]:
        if os.path.exists(p):
            size_mb = os.path.getsize(p) / 1024 / 1024
            print(f"  {os.path.basename(p)}: {size_mb:.1f} MB")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifacts-dir", default="./artifacts")
    args = parser.parse_args()

    os.makedirs(args.artifacts_dir, exist_ok=True)

    # Verify ezkl is importable and show version
    print(f"[setup] ezkl version : {ezkl.__version__}")
    print(f"[setup] Python       : {sys.version}")
    print(f"[setup] artifacts dir: {args.artifacts_dir}")
    print()

    asyncio.run(run_setup(args.artifacts_dir))