"""
worker.py
---------
Main prover worker — spawned by Rust prover-manager per proof job.

Phase 1: generates individual proofs (ezkl v23.0.5)
Phase 2: aggregation uses separate .venv-aggr (ezkl v15.6.3)
         — this file does NOT change for aggregation.

Protocol (stdin/stdout JSON):

stdin:
{
  "job_id":        "uuid",
  "input_data":    [[0.1, 0.2, 0.3, 0.4]],
  "artifacts_dir": "./artifacts"
}

stdout (success):
{ "job_id": "uuid", "status": "ok", "proof_path": "/tmp/{job_id}_proof.json" }

stdout (failure):
{ "job_id": "uuid", "status": "error", "error": "reason" }

exit codes: 0 = success, 1 = failure
"""

import json
import os
import sys
import tempfile

import ezkl


def prove(job: dict) -> dict:
    job_id        = job["job_id"]
    input_data    = job["input_data"]
    artifacts_dir = job.get("artifacts_dir", "./artifacts")

    compiled_path = os.path.join(artifacts_dir, "model.compiled")
    pk_path       = os.path.join(artifacts_dir, "pk.key")
    vk_path       = os.path.join(artifacts_dir, "vk.key")
    settings_path = os.path.join(artifacts_dir, "settings.json")

    for path in [compiled_path, pk_path, vk_path, settings_path]:
        if not os.path.exists(path):
            return {
                "job_id": job_id,
                "status": "error",
                "error":  f"missing artifact: {path} — run setup_circuit.py first",
            }

    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False, dir="/tmp"
    ) as f:
        json.dump({"input_data": input_data}, f)
        input_path = f.name

    witness_path = f"/tmp/{job_id}_witness.json"
    proof_path   = f"/tmp/{job_id}_proof.json"

    try:
        res = ezkl.gen_witness(input_path, compiled_path, witness_path)
        if not res:
            return {"job_id": job_id, "status": "error", "error": "witness generation failed"}

        # v23.0.5 — no proof_type parameter, standard prove() call
        res = ezkl.prove(witness_path, compiled_path, pk_path, proof_path)
        if not res:
            return {"job_id": job_id, "status": "error", "error": "proof generation failed"}

        res = ezkl.verify(proof_path, settings_path, vk_path)
        if not res:
            return {"job_id": job_id, "status": "error", "error": "local verification failed"}

        return {"job_id": job_id, "status": "ok", "proof_path": proof_path}

    except Exception as e:
        return {"job_id": job_id, "status": "error", "error": str(e)}

    finally:
        for path in [input_path, witness_path]:
            if os.path.exists(path):
                os.unlink(path)


def main() -> None:
    raw = sys.stdin.read().strip()
    if not raw:
        print(json.dumps({"status": "error", "error": "no input on stdin"}))
        sys.exit(1)

    try:
        job = json.loads(raw)
    except json.JSONDecodeError as e:
        print(json.dumps({"status": "error", "error": f"invalid JSON: {e}"}))
        sys.exit(1)

    result = prove(job)
    print(json.dumps(result), flush=True)
    sys.exit(0 if result["status"] == "ok" else 1)


if __name__ == "__main__":
    main()