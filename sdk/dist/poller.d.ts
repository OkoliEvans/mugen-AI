import { AxiosInstance } from 'axios';
import { Job } from './types';
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
export declare function pollUntilDone(client: AxiosInstance, jobId: string, timeoutMs: number, intervalMs: number): Promise<Job>;
//# sourceMappingURL=poller.d.ts.map