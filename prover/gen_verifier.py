"""
gen_verifier.py
---------------
Generates the Solidity verifier contract from the circuit's verification key.
Run this after setup_circuit.py, before deploying to Base Sepolia.

Uses the ezkl Python API directly (no CLI binary required).

Usage:
    python3 gen_verifier.py [--artifacts-dir ./artifacts] [--contracts-dir ../contracts/src]
"""

import argparse
import os
import sys
import asyncio

import ezkl


def gen_verifier(artifacts_dir: str, contracts_dir: str) -> None:
    os.makedirs(contracts_dir, exist_ok=True)

    vk_path       = os.path.join(artifacts_dir, "vk.key")
    settings_path = os.path.join(artifacts_dir, "settings.json")
    sol_path      = os.path.join(contracts_dir, "Verifier.sol")
    abi_path      = os.path.join(contracts_dir, "Verifier.abi")

    for path in [vk_path, settings_path]:
        if not os.path.exists(path):
            print(f"[gen_verifier] ERROR: not found: {path}")
            print("[gen_verifier] Run setup_circuit.py first.")
            sys.exit(1)

    print(f"[gen_verifier] ezkl version  : {ezkl.__version__}")
    print(f"[gen_verifier] vk.key        : {vk_path}")
    print(f"[gen_verifier] settings.json : {settings_path}")
    print(f"[gen_verifier] output dir    : {contracts_dir}")
    print()
    print("[gen_verifier] Generating Solidity verifier...")

    try:
        async def _run():
            return ezkl.create_evm_verifier(
                vk_path=vk_path,
                settings_path=settings_path,
                sol_code_path=sol_path,
                abi_path=abi_path,
                reusable=False,
            )
        res = asyncio.run(_run())
    except Exception as e:
        print(f"[gen_verifier] FAILED: {e}", file=sys.stderr)
        sys.exit(1)

    if not res:
        print("[gen_verifier] FAILED — returned False", file=sys.stderr)
        sys.exit(1)

    print()
    for label, path in [("Verifier.sol", sol_path), ("Verifier.abi", abi_path)]:
        if os.path.exists(path):
            kb = os.path.getsize(path) / 1024
            print(f"[gen_verifier] {label} — {kb:.1f} KB → {path}")
        else:
            print(f"[gen_verifier] WARNING: {label} not found at {path}")

    print()
    print("[gen_verifier] Next step — deploy to Base Sepolia:")
    print(f"  forge create {sol_path}:Halo2Verifier \\")
    print(f"    --rpc-url https://sepolia.base.org \\")
    print(f"    --private-key $PRIVATE_KEY")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Generate Solidity verifier from vk.key")
    parser.add_argument("--artifacts-dir", default="./artifacts")
    parser.add_argument("--contracts-dir", default="../contracts/src")
    args = parser.parse_args()

    gen_verifier(args.artifacts_dir, args.contracts_dir)