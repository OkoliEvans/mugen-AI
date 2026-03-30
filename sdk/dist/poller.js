"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.pollUntilDone = pollUntilDone;
const errors_1 = require("./errors");
const http_1 = require("./http");
const TERMINAL_STATES = new Set(['done', 'failed']);
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
async function pollUntilDone(client, jobId, timeoutMs, intervalMs) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
        const job = await (0, http_1.safeRequest)(async () => {
            const { data } = await client.get(`/v1/jobs/${jobId}`);
            return data;
        }, 'POLL_FAILED', `polling job ${jobId}`);
        if (TERMINAL_STATES.has(job.status)) {
            if (job.status === 'failed') {
                throw new errors_1.ElenxisError('JOB_FAILED', `job ${jobId} failed: ${job.reason ?? 'unknown reason'}`);
            }
            return job;
        }
        await sleep(intervalMs);
    }
    throw new errors_1.ElenxisError('TIMEOUT', `job ${jobId} did not complete within ${timeoutMs}ms`);
}
function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
//# sourceMappingURL=poller.js.map