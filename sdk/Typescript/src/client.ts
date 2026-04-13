import { AxiosInstance } from 'axios';
import { createHttpClient, safeRequest } from './http';
import { pollUntilDone } from './poller';
import {
  VeilConfig,
  Job,
  JobStatus,
  ProofData,
  VerifyResult,
} from './types';
import { VeilError } from './errors';

const DEFAULT_TIMEOUT_MS    = 300_000;
const DEFAULT_POLL_INTERVAL = 4_000;
const DEFAULT_MAX_RETRIES   = 3;

/** Raw job shape returned by the gateway (snake_case JSON) */
interface RawJob {
  job_id:           string;
  status:           JobStatus;
  proof_path?:      string;
  tx_hash?:         string;
  attestation_hash?: string;
  reason?:          string;
}

/** Maps gateway snake_case response to SDK camelCase Job */
function mapJob(raw: RawJob): Job {
  return {
    jobId:           raw.job_id,
    status:          raw.status,
    proofPath:       raw.proof_path,
    txHash:          raw.tx_hash,
    attestationHash: raw.attestation_hash,
    reason:          raw.reason,
  };
}

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
export class VeilClient {
  private readonly http:           AxiosInstance;
  private readonly timeoutMs:      number;
  private readonly pollIntervalMs: number;

  constructor(config: VeilConfig) {
    if (!config.gatewayUrl) {
      throw new VeilError('NETWORK_ERROR', 'gatewayUrl is required');
    }

    this.http           = createHttpClient(config.gatewayUrl, config.maxRetries ?? DEFAULT_MAX_RETRIES);
    this.timeoutMs      = config.timeoutMs      ?? DEFAULT_TIMEOUT_MS;
    this.pollIntervalMs = config.pollIntervalMs ?? DEFAULT_POLL_INTERVAL;
  }

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
  async verifyInference(params: {
    modelId:   string;
    inputData: number[][];
  }): Promise<VerifyResult> {
    const startMs = Date.now();

    // 1. Submit job
    const jobId = await this.submitJob(params);

    // 2. Poll until done — terminal once compressed proof is ready.
    //    txHash is NOT awaited here — it arrives after batch settlement.
    const job = await pollUntilDone(
      this.http,
      jobId,
      this.timeoutMs,
      this.pollIntervalMs
    );

    // 3. Derive attestation hash from the job status response.
    //    attestation_hash is written to Postgres when status = "proving"
    //    and preserved on the "done" row. No separate proof fetch needed.
    const attestationHash = await this.fetchAttestationHash(jobId);

    return {
      jobId,
      status:          job.status === 'done' ? 'verified' : job.status as VerifyResult['status'],
      attestationHash,
      txHash:          job.txHash,   // undefined until batch settles — this is expected
      elapsedMs:       Date.now() - startMs,
    };
  }

  /**
   * Submit a job without waiting for completion.
   * Use this for fire-and-forget flows where you'll poll manually.
   *
   * @returns The jobId to track with waitForJob() or getJob()
   */
  async submitJob(params: {
    modelId:   string;
    inputData: number[][];
  }): Promise<string> {
    return safeRequest(
      async () => {
        const { data } = await this.http.post<{ job_id: string }>('/v1/jobs', {
          model_id:   params.modelId,
          input_data: params.inputData,
        });
        return data.job_id;
      },
      'SUBMIT_FAILED',
      'submitting inference job'
    );
  }

  /**
   * Get the current status of a job.
   */
  async getJob(jobId: string): Promise<Job> {
    return safeRequest(
      async () => {
        const { data } = await this.http.get<RawJob>(`/v1/jobs/${jobId}`);
        return mapJob(data);
      },
      'POLL_FAILED',
      `fetching job ${jobId}`
    );
  }

  /**
   * Wait for an existing job to complete.
   * Useful when combined with submitJob() for manual control.
   */
  async waitForJob(jobId: string): Promise<Job> {
    return pollUntilDone(
      this.http,
      jobId,
      this.timeoutMs,
      this.pollIntervalMs
    );
  }

  /**
   * Fetch proof metadata for a completed job.
   *
   * Note: per-job Groth16 proof bytes no longer exist. This endpoint returns
   * the attestation_hash and batch queue status for the job.
   */
  async getProof(jobId: string): Promise<ProofData> {
    return safeRequest(
      async () => {
        const { data } = await this.http.get<{
          job_id:           string;
          attestation_hash: string;
          status:           string;
          note?:            string;
        }>(`/v1/jobs/${jobId}/proof`);

        return {
          jobId:    data.job_id,
          // proofHex repurposed — carries attestation_hash for this endpoint.
          // Full Groth16 bytes are aggregated per batch, not per job.
          proofHex:  data.attestation_hash ?? '',
          sizeBytes: 0,
        };
      },
      'PROOF_FETCH_FAILED',
      `fetching proof metadata for job ${jobId}`
    );
  }

  /**
   * Check gateway liveness.
   * @returns true if the gateway is reachable and healthy
   */
  async healthCheck(): Promise<boolean> {
    try {
      const { data } = await this.http.get<{ status: string }>('/healthz');
      return data.status === 'ok';
    } catch {
      return false;
    }
  }

  // ── Private helpers ────────────────────────────────────────────────────────

  /**
   * Fetch attestation_hash for a completed job directly from the job status.
   * The gateway writes attestation_hash to Postgres when status = "proving"
   * and it is preserved on the "done" row — no proof bytes needed.
   */
  private async fetchAttestationHash(jobId: string): Promise<string> {
    try {
      const job = await this.getJob(jobId);
      if (job.attestationHash) return job.attestationHash;

      // Fallback: try the proof endpoint which also carries attestation_hash
      const proof = await this.getProof(jobId);
      return proof.proofHex || '';
    } catch {
      return '';
    }
  }
}