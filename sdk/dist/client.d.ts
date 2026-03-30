import { ElenxisConfig, Job, ProofData, VerifyResult } from './types';
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
export declare class ElenxisClient {
    private readonly http;
    private readonly timeoutMs;
    private readonly pollIntervalMs;
    constructor(config: ElenxisConfig);
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
    verifyInference(params: {
        modelId: string;
        inputData: number[][];
    }): Promise<VerifyResult>;
    /**
     * Submit a job without waiting for completion.
     * Use this for fire-and-forget flows where you'll poll manually.
     *
     * @returns The jobId to track with pollJob() or getJob()
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
     * Fetches the attestation hash for a completed job.
     * The attestation hash is the keccak256 of the job output stored on-chain.
     * We derive it from the job status response.
     */
    private fetchAttestationHash;
}
//# sourceMappingURL=client.d.ts.map