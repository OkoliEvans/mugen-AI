type JobStatus = "queued" | "running" | "done" | "settled" | "failed";
interface Job {
    /** Unique job identifier (UUID) */
    jobId: string;
    /** Current lifecycle status */
    status: JobStatus;
    /** Path to proof file on the prover node (populated when done) */
    proofPath?: string;
    /** On-chain transaction hash (populated after settlement) */
    txHash?: string;
    attestationHash?: string;
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
    status: "settled" | "pending" | "failed";
}
interface VerifyResult {
    /** Unique job identifier */
    jobId: string;
    /** Status of the verification */
    status: "pending" | "verified" | "failed";
    /** keccak256 of the output data used as the attestation key on-chain */
    attestationHash: string;
    /** On-chain transaction hash of the settled proof */
    txHash?: string;
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
interface VeilConfig {
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
type VeilErrorCode = "SUBMIT_FAILED" | "POLL_FAILED" | "JOB_FAILED" | "TIMEOUT" | "PROOF_FETCH_FAILED" | "NETWORK_ERROR";

/**
 * VeilClient — the primary entry point for the Veil SDK.
 *
 * @example
 * ```typescript
 * import { VeilClient } from '@mugen-ai/sdk';
 *
 * const client = new VeilClient({
 *   gatewayUrl: 'http://localhost:8080',
 * });
 *
 * const result = await client.verifyInference({
 *   modelId:   'polymarket_mlp_v1',
 *   inputData: [[0.6, 0.4, 12000, 0.2]],
 * });
 *
 * console.log(result.attestationHash); // keccak256 proof fingerprint
 * console.log(result.txHash);          // HashKey testnet tx — available after batch settlement
 * ```
 */
declare class VeilClient {
    private readonly http;
    private readonly timeoutMs;
    private readonly pollIntervalMs;
    constructor(config: VeilConfig);
    /**
     * Submit an inference for ZK verification and wait for the compressed proof.
     *
     * This is the primary SDK method. It:
     *   1. Submits the inference job to the gateway
     *   2. Polls until the job reaches 'done' status (compressed proof ready)
     *   3. Returns attestationHash immediately — available as soon as proving completes
     *
     * Note: txHash is populated asynchronously after batch settlement. The aggregator
     * collects N compressed proofs then runs one Groth16 for on-chain settlement.
     * txHash will be undefined until that batch settles. Poll getJob(jobId) to
     * check for it, or query isVerified() on the contract directly.
     *
     * @param params.modelId   - Model identifier (must match a registered model)
     * @param params.inputData - 2D array of input values matching the model's input shape
     * @returns VerifyResult containing jobId, attestationHash, optional txHash, and elapsedMs
     * @throws VeilError on submission failure, job failure, or timeout
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
     * Fetch proof metadata for a completed job.
     *
     * Note: per-job Groth16 proof bytes no longer exist. This endpoint returns
     * the attestation_hash and batch queue status for the job.
     */
    getProof(jobId: string): Promise<ProofData>;
    /**
     * Check gateway liveness.
     * @returns true if the gateway is reachable and healthy
     */
    healthCheck(): Promise<boolean>;
    /**
     * Fetch attestation_hash for a completed job directly from the job status.
     * The gateway writes attestation_hash to Postgres when status = "proving"
     * and it is preserved on the "done" row — no proof bytes needed.
     */
    private fetchAttestationHash;
}

/**
 * All errors thrown by the Veil SDK are instances of VeilError.
 * Check the `code` field to handle specific failure modes.
 *
 * @example
 * try {
 *   await client.verifyInference(...)
 * } catch (err) {
 *   if (err instanceof VeilError && err.code === 'TIMEOUT') {
 *     // handle timeout
 *   }
 * }
 */
declare class VeilError extends Error {
    readonly code: VeilErrorCode;
    readonly cause?: unknown;
    constructor(code: VeilErrorCode, message: string, cause?: unknown);
}

export { type Attestation, type Job, type JobStatus, type ProofData, VeilClient, type VeilConfig, VeilError, type VeilErrorCode, type VerifyResult };
