import { AxiosInstance } from "axios";
import { Job, JobStatus } from "./types";
import { VeilError } from "./errors";
import { safeRequest } from "./http";

// 'settled' is the terminal success state for StarkNet settlement path —
// tx_hash is only written once the L1→L2 relay completes and the gateway
// updates status from 'done' (proof ready) to 'settled' (on-chain confirmed).
// 'done' is kept as a terminal state for the EVM-only fast path where
// tx_hash is written in the same status update.
const TERMINAL_STATES = new Set<Job["status"]>(["done", "settled", "failed"]);

interface RawJob {
  job_id: string;
  status: JobStatus;
  proof_path?: string;
  tx_hash?: string;
  reason?: string;
  settlement_chain?: string;
}

function mapJob(raw: RawJob): Job {
  return {
    jobId: raw.job_id,
    status: raw.status,
    proofPath: raw.proof_path,
    txHash: raw.tx_hash,
    reason: raw.reason,
  };
}

/**
 * Polls GET /v1/jobs/:jobId until the job reaches a terminal state
 * (done, settled, or failed), or until the timeout is exceeded.
 *
 * Terminal state behaviour:
 *   - 'done'    — proof generated and EVM tx confirmed (EVM settlement paths)
 *   - 'settled' — proof confirmed on destination chain (all paths, including StarkNet)
 *   - 'failed'  — job failed at any stage
 *
 * For StarkNet settlement, the gateway holds the job in 'running'/'done'
 * while the L1→L2 relay completes, then flips to 'settled' with tx_hash.
 * Increase timeoutMs to ~300_000 when using settlementChain: 'starknet'.
 *
 * @param client     - Pre-configured Axios instance
 * @param jobId      - Job ID to poll
 * @param timeoutMs  - Maximum total wait time in milliseconds
 * @param intervalMs - Polling interval in milliseconds
 * @returns The completed Job record
 * @throws ElenxisError with code TIMEOUT if the job doesn't complete in time
 * @throws ElenxisError with code JOB_FAILED if the job transitions to failed
 */
export async function pollUntilDone(
  client: AxiosInstance,
  jobId: string,
  timeoutMs: number,
  intervalMs: number,
): Promise<Job> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    const job = await safeRequest(
      async () => {
        const { data } = await client.get<RawJob>(`/v1/jobs/${jobId}`);
        return mapJob(data);
      },
      "POLL_FAILED",
      `polling job ${jobId}`,
    );

    if (TERMINAL_STATES.has(job.status)) {
      if (job.status === "failed") {
        throw new VeilError(
          "JOB_FAILED",
          `job ${jobId} failed: ${job.reason ?? "unknown reason"}`,
        );
      }
      if (job.status === "done" && !job.txHash) {
        await sleep(intervalMs);
        continue;
      }

      return job;
    }

    await sleep(intervalMs);
  }

  throw new VeilError(
    "TIMEOUT",
    `job ${jobId} did not complete within ${timeoutMs}ms`,
  );
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
