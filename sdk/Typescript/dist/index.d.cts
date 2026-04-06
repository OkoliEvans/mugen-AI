type JobStatus = 'queued' | 'running' | 'done' | 'settled' | 'failed';
interface Job {
    /** Unique job identifier (UUID) */
    jobId: string;
    /** Current lifecycle status */
    status: JobStatus;
    /** Path to proof file on the prover node (populated when done) */
    proofPath?: string;
    /** On-chain transaction hash (populated after settlement) */
    txHash?: string;
    /** Failure reason (populated when failed) */
    reason?: string;
}
interface Attestation {
    /** Job that produced this attestation */
    jobId: string;
    /** keccak256 of the model identifier */
    modelId: string;
    /** On-chain transaction hash of the settlement */
    txHash: string;
    /** keccak256 of the output data — the on-chain attestation key */
    outputHash: string;
    /** Status of the on-chain proof */
    status: 'settled' | 'pending' | 'failed';
}
interface VerifyResult {
    /** Unique job identifier */
    jobId: string;
    /** keccak256 of the output data used as the attestation key on-chain */
    attestationHash: string;
    /** On-chain transaction hash of the settled proof */
    txHash: string;
    /** Time taken from submission to settlement (milliseconds) */
    elapsedMs: number;
}
interface ProofData {
    jobId: string;
    /** Full proof as a hex string */
    proofHex: string;
    /** Proof size in bytes */
    sizeBytes: number;
}
interface ElenxisConfig {
    /** Gateway base URL — e.g. http://localhost:8080 */
    gatewayUrl: string;
    /**
     * How long to wait for a job to complete before throwing (ms).
     * Default: 120_000 (2 minutes)
     */
    timeoutMs?: number;
    /**
     * How often to poll job status (ms).
     * Default: 1_000
     */
    pollIntervalMs?: number;
    /**
     * Number of times to retry a failed HTTP request.
     * Default: 3
     */
    maxRetries?: number;
}
type ElenxisErrorCode = 'SUBMIT_FAILED' | 'POLL_FAILED' | 'JOB_FAILED' | 'TIMEOUT' | 'PROOF_FETCH_FAILED' | 'NETWORK_ERROR';

/**
 * ElenxisClient — the primary entry point for the Elenxis SDK.
 *
 * @example
 * ```typescript
 * import { ElenxisClient } from '@elenxis/sdk';
 *
 * // EVM settlement (default)
 * const client = new ElenxisClient({
 *   gatewayUrl: 'http://localhost:8080',
 * });
 *
 * // StarkNet settlement — increase timeout to account for L1→L2 relay
 * const client = new ElenxisClient({
 *   gatewayUrl: 'http://localhost:8080',
 *   timeoutMs:  300_000,
 * });
 *
 * const result = await client.verifyInference({
 *   modelId:   'tiny_mlp_v1',
 *   inputData: [[0.1, 0.2, 0.3, 0.4]],
 * });
 *
 * console.log(result.txHash);          // on-chain tx hash
 * console.log(result.attestationHash); // keccak256 output hash stored on-chain
 * ```
 */
declare class ElenxisClient {
    private readonly http;
    private readonly timeoutMs;
    private readonly pollIntervalMs;
    constructor(config: ElenxisConfig);
    /**
     * Submit an inference for ZK verification and wait for on-chain settlement.
     *
     * This is the primary SDK method. It:
     *   1. Submits the inference job to the gateway
     *   2. Polls until the job reaches 'done' or 'settled' status
     *   3. Fetches tx_hash from Postgres (retries until available)
     *   4. Returns the attestation hash and transaction reference
     *
     * For StarkNet settlement, set timeoutMs to at least 300_000 (5 min)
     * to account for the L1→L2 relay latency.
     *
     * @param params.modelId   - Model identifier (must match a registered model)
     * @param params.inputData - 2D array of input values matching the model's input shape
     * @returns VerifyResult containing jobId, attestationHash, txHash, and elapsedMs
     * @throws ElenxisError on submission failure, job failure, or timeout
     */
    verifyInference(params: {
        modelId: string;
        inputData: number[][];
    }): Promise<VerifyResult>;
    /**
     * Submit a job without waiting for completion.
     * Use this for fire-and-forget flows where you'll poll manually.
     *
     * @returns The jobId to track with waitForJob() or getJob()
     */
    submitJob(params: {
        modelId: string;
        inputData: number[][];
    }): Promise<string>;
    /**
     * Get the current status of a job.
     */
    getJob(jobId: string): Promise<Job>;
    /**
     * Wait for an existing job to complete.
     * Useful when combined with submitJob() for manual control.
     */
    waitForJob(jobId: string): Promise<Job>;
    /**
     * Fetch the raw proof bytes for a completed job.
     */
    getProof(jobId: string): Promise<ProofData>;
    /**
     * Check gateway liveness.
     * @returns true if the gateway is reachable and healthy
     */
    healthCheck(): Promise<boolean>;
    /**
     * Derives the attestation hash for a completed job from its proof bytes.
     * Returns the first 32 bytes of the proof hex as a fingerprint.
     * Full on-chain verification should use isVerified() on the contract directly.
     */
    private fetchAttestationHash;
}

/**
 * All errors thrown by the Elenxis SDK are instances of ElenxisError.
 * Check the `code` field to handle specific failure modes.
 *
 * @example
 * try {
 *   await client.verifyInference(...)
 * } catch (err) {
 *   if (err instanceof ElenxisError && err.code === 'TIMEOUT') {
 *     // handle timeout
 *   }
 * }
 */
declare class ElenxisError extends Error {
    readonly code: ElenxisErrorCode;
    readonly cause?: unknown;
    constructor(code: ElenxisErrorCode, message: string, cause?: unknown);
}

export { type Attestation, ElenxisClient, type ElenxisConfig, ElenxisError, type ElenxisErrorCode, type Job, type JobStatus, type ProofData, type VerifyResult };
