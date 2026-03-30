"""
gen_agg_verifier.py
-------------------
Generates the Solidity aggregate verifier contract from agg_vk.key.

Run AFTER aggregator.py has run at least once (so agg_vk.key exists).
Run BEFORE deploying AggregatedVerifier.sol.

Usage:
    python gen_agg_verifier.py \
        [--artifacts-dir ./artifacts] \
        [--contracts-dir ../contracts/src] \
        [--logrows 23]
"""

import argparse
import asyncio
import os
import sys

import ezkl


async def run(artifacts_dir: str, contracts_dir: str, logrows: int) -> None:
    os.makedirs(contracts_dir, exist_ok=True)

    agg_vk_path   = os.path.join(artifacts_dir, "agg_vk.key")
    settings_path = os.path.join(artifacts_dir, "settings.json")
    sol_path      = os.path.join(contracts_dir, "Halo2AggVerifier.sol")
    abi_path      = os.path.join(contracts_dir, "Halo2AggVerifier.abi")

    # agg_vk.key only exists after aggregator.py has run setup_aggregate once
    if not os.path.exists(agg_vk_path):
        print(
            f"[gen_agg_verifier] ERROR: agg_vk.key not found at {agg_vk_path}\n"
            f"  Run aggregator.py with at least one batch first to generate it.",
            file=sys.stderr,
        )
        sys.exit(1)

    if not os.path.exists(settings_path):
        print(
            f"[gen_agg_verifier] ERROR: settings.json not found at {settings_path}",
            file=sys.stderr,
        )
        sys.exit(1)

    print(f"[gen_agg_verifier] ezkl version : {ezkl.__version__}")
    print(f"[gen_agg_verifier] agg_vk.key   : {agg_vk_path}")
    print(f"[gen_agg_verifier] logrows       : {logrows}")
    print(f"[gen_agg_verifier] output        : {sol_path}")
    print()

    print("[gen_agg_verifier] generating Halo2AggVerifier.sol...")
    try:
        # aggregation_settings accepts a list of settings paths
        # (one per circuit type — we have one TinyMLP circuit)
        res = await ezkl.create_evm_verifier_aggr(
            aggregation_settings=[settings_path],
            vk_path=agg_vk_path,
            sol_code_path=sol_path,
            abi_path=abi_path,
            logrows=logrows,
            reusable=False,
        )
    except Exception as e:
        print(f"[gen_agg_verifier] FAILED: {e}", file=sys.stderr)
        sys.exit(1)

    if not res:
        print("[gen_agg_verifier] FAILED — returned False", file=sys.stderr)
        sys.exit(1)

    for label, path in [("Halo2AggVerifier.sol", sol_path), ("Halo2AggVerifier.abi", abi_path)]:
        if os.path.exists(path):
            kb = os.path.getsize(path) / 1024
            print(f"[gen_agg_verifier] {label} — {kb:.1f} KB → {path}")
        else:
            print(f"[gen_agg_verifier] WARNING: {label} not written to {path}")

    print()
    print("[gen_agg_verifier] Next steps:")
    print(f"  1. Deploy Halo2AggVerifier:")
    print(f"     forge create {sol_path}:Halo2VerifyingAggregation \\")
    print(f"       --rpc-url $RPC_URL --private-key $PRIVATE_KEY")
    print()
    print(f"  2. Set HALO2_AGG_VERIFIER_ADDRESS in .env to the deployed address")
    print()
    print(f"  3. Deploy AggregatedVerifier:")
    print(f"     forge script script/Deploy.s.sol:Deploy \\")
    print(f"       --sig 'deployAggregated()' \\")
    print(f"       --rpc-url $RPC_URL --private-key $PRIVATE_KEY --broadcast")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Generate Halo2AggVerifier.sol from agg_vk.key"
    )
    parser.add_argument("--artifacts-dir", default="./artifacts")
    parser.add_argument("--contracts-dir", default="../contracts/src")
    parser.add_argument("--logrows", type=int, default=23)
    args = parser.parse_args()

    asyncio.run(run(args.artifacts_dir, args.contracts_dir, args.logrows))