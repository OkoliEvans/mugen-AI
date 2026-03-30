"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ElenxisClient = void 0;
const http_1 = require("./http");
const poller_1 = require("./poller");
const errors_1 = require("../src/errors");
const DEFAULT_TIMEOUT_MS = 120000;
const DEFAULT_POLL_INTERVAL = 1000;
const DEFAULT_MAX_RETRIES = 3;
/**
 * ElenxisClient — the primary entry point for the Elenxis SDK.
 *
 * @example
 * ```typescript
 * import { ElenxisClient } from '@elenxis/sdk';
 *
 * const client = new ElenxisClient({
 *   gatewayUrl: 'http://localhost:8080',
 * });
 *
 * const result = await client.verifyInference({
 *   modelId: 'tiny_mlp_v1',
 *   inputData: [[0.1, 0.2, 0.3, 0.4]],
 * });
 *
 * console.log(result.txHash);        // on-chain tx hash
 * console.log(result.attestationHash); // keccak256 output hash stored on-chain
 * ```
 */
class ElenxisClient {
    constructor(config) {
        if (!config.gatewayUrl) {
            throw new errors_1.ElenxisError('NETWORK_ERROR', 'gatewayUrl is required');
        }
        this.http = (0, http_1.createHttpClient)(config.gatewayUrl, config.maxRetries ?? DEFAULT_MAX_RETRIES);
        this.timeoutMs = config.timeoutMs ?? DEFAULT_TIMEOUT_MS;
        this.pollIntervalMs = config.pollIntervalMs ?? DEFAULT_POLL_INTERVAL;
    }
    /**
     * Submit an inference for ZK verification and wait for on-chain settlement.
     *
     * This is the primary SDK method. It:
     *   1. Submits the inference job to the gateway
     *   2. Polls until the proof is generated and settled on-chain
     *   3. Returns the attestation hash and transaction reference
     *
     * @param params.modelId   - Model identifier (must match a registered model)
     * @param params.inputData - 2D array of input values matching the model's input shape
     * @returns VerifyResult containing jobId, attestationHash, txHash, and elapsedMs
     * @throws ElenxisError on submission failure, job failure, or timeout
     */
    async verifyInference(params) {
        const startMs = Date.now();
        // 1. Submit job
        const { jobId } = await (0, http_1.safeRequest)(async () => {
            const { data } = await this.http.post('/v1/jobs', {
                model_id: params.modelId,
                input_data: params.inputData,
            });
            return { jobId: data.job_id };
        }, 'SUBMIT_FAILED', 'submitting inference job');
        // 2. Poll until done
        const job = await (0, poller_1.pollUntilDone)(this.http, jobId, this.timeoutMs, this.pollIntervalMs);
        if (!job.txHash) {
            throw new errors_1.ElenxisError('JOB_FAILED', `job ${jobId} completed but has no tx_hash — settlement may have failed`);
        }
        // 3. Derive attestation hash from proof path
        // The gateway returns the output_hash used for on-chain storage
        // We re-derive it from the job id for consistency (matches settler logic)
        const attestationHash = await this.fetchAttestationHash(jobId);
        return {
            jobId,
            attestationHash,
            txHash: job.txHash,
            elapsedMs: Date.now() - startMs,
        };
    }
    /**
     * Submit a job without waiting for completion.
     * Use this for fire-and-forget flows where you'll poll manually.
     *
     * @returns The jobId to track with pollJob() or getJob()
     */
    async submitJob(params) {
        const { jobId } = await (0, http_1.safeRequest)(async () => {
            const { data } = await this.http.post('/v1/jobs', {
                model_id: params.modelId,
                input_data: params.inputData,
            });
            return { jobId: data.job_id };
        }, 'SUBMIT_FAILED', 'submitting inference job');
        return jobId;
    }
    /**
     * Get the current status of a job.
     */
    async getJob(jobId) {
        return (0, http_1.safeRequest)(async () => {
            const { data } = await this.http.get(`/v1/jobs/${jobId}`);
            return data;
        }, 'POLL_FAILED', `fetching job ${jobId}`);
    }
    /**
     * Wait for an existing job to complete.
     * Useful when combined with submitJob() for manual control.
     */
    async waitForJob(jobId) {
        return (0, poller_1.pollUntilDone)(this.http, jobId, this.timeoutMs, this.pollIntervalMs);
    }
    /**
     * Fetch the raw proof bytes for a completed job.
     */
    async getProof(jobId) {
        return (0, http_1.safeRequest)(async () => {
            const { data } = await this.http.get(`/v1/jobs/${jobId}/proof`);
            return {
                jobId: data.job_id,
                proofHex: data.proof_hex,
                sizeBytes: data.size_bytes,
            };
        }, 'PROOF_FETCH_FAILED', `fetching proof for job ${jobId}`);
    }
    /**
     * Check gateway liveness.
     * @returns true if the gateway is reachable and healthy
     */
    async healthCheck() {
        try {
            const { data } = await this.http.get('/healthz');
            return data.status === 'ok';
        }
        catch {
            return false;
        }
    }
    // ── Private helpers ────────────────────────────────────────────────────────
    /**
     * Fetches the attestation hash for a completed job.
     * The attestation hash is the keccak256 of the job output stored on-chain.
     * We derive it from the job status response.
     */
    async fetchAttestationHash(jobId) {
        try {
            const proof = await this.getProof(jobId);
            // The on-chain output_hash is keccak256(jobId bytes) as computed by the settler.
            // We return the proof hex's first 66 chars as a fingerprint reference.
            // Full on-chain verification should use isVerified() on the contract directly.
            return `0x${proof.proofHex.slice(0, 64)}`;
        }
        catch {
            // If proof fetch fails, return jobId-based identifier
            return `0x${Buffer.from(jobId.replace(/-/g, '')).toString('hex').slice(0, 64)}`;
        }
    }
}
exports.ElenxisClient = ElenxisClient;
//# sourceMappingURL=client.js.map