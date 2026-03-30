import { AxiosInstance } from 'axios';
import { Job, JobStatus } from './types';
import { ElenxisError } from './errors';
import { safeRequest } from './http';

const TERMINAL_STATES = new Set<Job['status']>(['done', 'failed']);

interface RawJob {
  job_id:      string;
  status:      JobStatus;
  proof_path?: string;
  tx_hash?:    string;
  reason?:     string;
}

function mapJob(raw: RawJob): Job {
  return {
    jobId:     raw.job_id,
    status:    raw.status,
    proofPath: raw.proof_path,
    txHash:    raw.tx_hash,
    reason:    raw.reason,
  };
}

/**
 * Polls GET /v1/jobs/:jobId until the job reaches a terminal state
 * (done or failed), or until the timeout is exceeded.
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
  intervalMs: number
): Promise<Job> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    const job = await safeRequest(
      async () => {
        const { data } = await client.get<RawJob>(`/v1/jobs/${jobId}`);
        return mapJob(data);
      },
      'POLL_FAILED',
      `polling job ${jobId}`
    );

    if (TERMINAL_STATES.has(job.status)) {
      if (job.status === 'failed') {
        throw new ElenxisError(
          'JOB_FAILED',
          `job ${jobId} failed: ${job.reason ?? 'unknown reason'}`
        );
      }
      return job;
    }

    await sleep(intervalMs);
  }

  throw new ElenxisError(
    'TIMEOUT',
    `job ${jobId} did not complete within ${timeoutMs}ms`
  );
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}