"""
aggregator.py
-------------
EZKL v15.6.3 aggregation worker — spawned by Rust aggregator crate.

Key fixes from official docs (https://pythonbindings.ezkl.xyz/en/v15.6.3):
  1. commitment must be string "kzg" not PyCommitments.KZG enum
  2. gen_witness returns dict, not bool — check with `is None` not `if not`
  3. prove returns bool — check normally
  4. run() must be async def so PyO3/Tokio event loop exists for all ezkl calls
  5. aggregation_settings in create_evm_verifier_aggr is a single str, not list

Protocol (stdin/stdout JSON):

stdin:
{
  "batch_id":      "uuid",
  "job_inputs":    [{"job_id": "uuid", "input_data": [[0.1, 0.2, 0.3, 0.4]]}, ...],
  "artifacts_dir": "./artifacts-aggr",
  "agg_artifacts": "./artifacts-aggr",
  "output_path":   "/tmp/batch_uuid_aggregated.json"
}

stdout (success):
{
  "status":                "ok",
  "aggregated_proof_path": "/tmp/batch_uuid_aggregated.json",
  "proof_count":           3,
  "size_kb":               12.4
}

stdout (failure):
{
  "status": "error",
  "error":  "reason"
}

exit codes: 0 = success, 1 = failure
"""

import asyncio
import json
import os
import sys
import tempfile

import ezkl

# From official docs: commitment accepts string "kzg" or "ipa" — NOT PyCommitments enum
COMMITMENT = "kzg"
LOGROWS    = 23   # aggregation circuit needs more rows than individual circuit (17)


def load_input() -> dict:
    raw = sys.stdin.read().strip()
    if not raw:
        return {"status": "error", "error": "no input on stdin"}
    try:
        return json.loads(raw)
    except Exception as e:
        return {"status": "error", "error": f"invalid JSON: {e}"}


async def run(job: dict) -> dict:
    """
    All ezkl calls must be inside an async function so PyO3's Tokio
    runtime has an event loop available. The functions are not awaited
    (they are sync from Python's perspective) but they require the
    event loop to exist in the thread.
    """
    batch_id      = job.get("batch_id", "unknown")
    job_inputs    = job.get("job_inputs", [])
    artifacts_dir = job.get("artifacts_dir", "./artifacts-aggr")
    agg_dir       = job.get("agg_artifacts", "./artifacts-aggr")
    output_path   = job.get("output_path", f"/tmp/batch_{batch_id}_aggregated.json")

    if not job_inputs:
        return {"status": "error", "error": "empty job_inputs"}

    os.makedirs(agg_dir, exist_ok=True)

    compiled_path = os.path.join(artifacts_dir, "model.compiled")
    pk_path       = os.path.join(artifacts_dir, "pk.key")

    for p in [compiled_path, pk_path]:
        if not os.path.exists(p):
            return {"status": "error", "error": f"missing artifact: {p}"}

    # ── Step 1: Re-prove each job with proof_type="for-aggr" ─────────────────
    proof_paths = []

    for item in job_inputs:
        job_id     = item["job_id"]
        input_data = item["input_data"]

        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".json", delete=False, dir="/tmp"
        ) as f:
            json.dump({"input_data": input_data}, f)
            input_path = f.name

        witness_path = f"/tmp/{job_id}_agg_witness.json"
        proof_path   = f"/tmp/{job_id}_aggr_proof.json"

        try:
            # gen_witness returns dict on success, raises on failure
            # official docs: ezkl.gen_witness(data, model, output, ...)
            result = ezkl.gen_witness(
                data=input_path,
                model=compiled_path,
                output=witness_path,
            )
            if result is None:
                return {
                    "status": "error",
                    "error":  f"gen_witness returned None for job {job_id}",
                }

            # prove returns bool
            # official docs: proof_type accepts "single" or "for-aggr"
            ok = ezkl.prove(
                witness=witness_path,
                model=compiled_path,
                pk_path=pk_path,
                proof_path=proof_path,
                proof_type="for-aggr",
            )
            if not ok:
                return {
                    "status": "error",
                    "error":  f"prove failed for job {job_id}",
                }

            proof_paths.append(proof_path)
            print(f"[aggregator] proved job {job_id}", file=sys.stderr)

        except Exception as e:
            return {"status": "error", "error": f"prove exception for {job_id}: {e}"}

        finally:
            for p in [input_path, witness_path]:
                if os.path.exists(p):
                    os.unlink(p)

    print(f"[aggregator] {len(proof_paths)} proofs ready for aggregation", file=sys.stderr)

    # ── Step 2: setup_aggregate (once — reuse if keys exist) ─────────────────
    agg_vk = os.path.join(agg_dir, "agg_vk.key")
    agg_pk = os.path.join(agg_dir, "agg_pk.key")

    if not (os.path.exists(agg_vk) and os.path.exists(agg_pk)):
        print("[aggregator] running setup_aggregate...", file=sys.stderr)
        try:
            ok = ezkl.setup_aggregate(
                sample_snarks=proof_paths[:1],
                vk_path=agg_vk,
                pk_path=agg_pk,
                logrows=LOGROWS,
                split_proofs=False,
                disable_selector_compression=False,
                commitment=COMMITMENT,          # string "kzg" per official docs
            )
            if not ok:
                return {"status": "error", "error": "setup_aggregate returned False"}
        except Exception as e:
            return {"status": "error", "error": f"setup_aggregate exception: {e}"}

        print("[aggregator] setup_aggregate done", file=sys.stderr)
    else:
        print("[aggregator] reusing existing agg_vk.key / agg_pk.key", file=sys.stderr)

    # ── Step 3: aggregate ─────────────────────────────────────────────────────
    print(f"[aggregator] aggregating {len(proof_paths)} proofs...", file=sys.stderr)
    try:
        ok = ezkl.aggregate(
            aggregation_snarks=proof_paths,
            proof_path=output_path,
            vk_path=agg_vk,
            transcript="evm",               # "evm" or "poseidon"
            logrows=LOGROWS,
            check_mode="UNSAFE",
            split_proofs=False,
            commitment=COMMITMENT,           # string "kzg" per official docs
        )
        if not ok:
            return {"status": "error", "error": "aggregate returned False"}
    except Exception as e:
        return {"status": "error", "error": f"aggregate exception: {e}"}

    print("[aggregator] aggregate done", file=sys.stderr)

    # ── Step 4: verify_aggr ───────────────────────────────────────────────────
    try:
        ok = ezkl.verify_aggr(
            proof_path=output_path,
            vk_path=agg_vk,
            logrows=LOGROWS,
            commitment=COMMITMENT,           # string "kzg" per official docs
            reduced_srs=False,
        )
        if not ok:
            return {"status": "error", "error": "verify_aggr returned False"}
    except Exception as e:
        return {"status": "error", "error": f"verify_aggr exception: {e}"}

    print("[aggregator] verify_aggr passed", file=sys.stderr)

    size_kb = os.path.getsize(output_path) / 1024 if os.path.exists(output_path) else 0.0

    return {
        "status":                "ok",
        "aggregated_proof_path": output_path,
        "proof_count":           len(proof_paths),
        "size_kb":               round(size_kb, 2),
    }


def main() -> None:
    job = load_input()
    if job.get("status") == "error":
        print(json.dumps(job), flush=True)
        sys.exit(1)

    result = asyncio.run(run(job))
    print(json.dumps(result), flush=True)
    sys.exit(0 if result["status"] == "ok" else 1)


if __name__ == "__main__":
    main()