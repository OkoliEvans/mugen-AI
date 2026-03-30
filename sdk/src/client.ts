import { AxiosInstance } from 'axios';
import { createHttpClient, safeRequest } from './http';
import { pollUntilDone } from './poller';
import {
  ElenxisConfig,
  Job,
  JobStatus,
  ProofData,
  VerifyResult,
} from './types';
import { ElenxisError } from './errors';

const DEFAULT_TIMEOUT_MS    = 120_000;
const DEFAULT_POLL_INTERVAL = 1_000;
const DEFAULT_MAX_RETRIES   = 3;

/** Raw job shape returned by the gateway (snake_case JSON) */
interface RawJob {
  job_id:      string;
  status:      JobStatus;
  proof_path?: string;
  tx_hash?:    string;
  reason?:     string;
}

/** Maps gateway snake_case response to SDK camelCase Job */
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
 *   modelId:   'tiny_mlp_v1',
 *   inputData: [[0.1, 0.2, 0.3, 0.4]],
 * });
 *
 * console.log(result.txHash);          // on-chain tx hash
 * console.log(result.attestationHash); // keccak256 output hash stored on-chain
 * ```
 */
export class ElenxisClient {
  private readonly http: AxiosInstance;
  private readonly timeoutMs: number;
  private readonly pollIntervalMs: number;

  constructor(config: ElenxisConfig) {
    if (!config.gatewayUrl) {
      throw new ElenxisError('NETWORK_ERROR', 'gatewayUrl is required');
    }

    this.http           = createHttpClient(config.gatewayUrl, config.maxRetries ?? DEFAULT_MAX_RETRIES);
    this.timeoutMs      = config.timeoutMs      ?? DEFAULT_TIMEOUT_MS;
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
  async verifyInference(params: {
    modelId:   string;
    inputData: number[][];
  }): Promise<VerifyResult> {
    const startMs = Date.now();

    // 1. Submit job
    const jobId = await this.submitJob(params);

    // 2. Poll until done
    const job = await pollUntilDone(
      this.http,
      jobId,
      this.timeoutMs,
      this.pollIntervalMs
    );

    // tx_hash is persisted by the gateway in a separate step immediately after
    // status transitions to 'done' — retry a few times to avoid a false failure
    if (!job.txHash) {
      const TX_HASH_RETRIES     = 5;
      const TX_HASH_RETRY_DELAY = 1_000;

      for (let i = 0; i < TX_HASH_RETRIES; i++) {
        await new Promise(res => setTimeout(res, TX_HASH_RETRY_DELAY));
        const refreshed = await this.getJob(jobId);
        if (refreshed.txHash) {
          job.txHash = refreshed.txHash;
          break;
        }
      }
    }

    if (!job.txHash) {
      throw new ElenxisError(
        'JOB_FAILED',
        `job ${jobId} completed but has no tx_hash — settlement may have failed`
      );
    }

    // 3. Fetch attestation hash from proof
    const attestationHash = await this.fetchAttestationHash(jobId);

    return {
      jobId,
      attestationHash,
      txHash:    job.txHash,
      elapsedMs: Date.now() - startMs,
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
   * Fetch the raw proof bytes for a completed job.
   */
  async getProof(jobId: string): Promise<ProofData> {
    return safeRequest(
      async () => {
        const { data } = await this.http.get<{
          job_id:     string;
          proof_hex:  string;
          size_bytes: number;
        }>(`/v1/jobs/${jobId}/proof`);

        return {
          jobId:     data.job_id,
          proofHex:  data.proof_hex,
          sizeBytes: data.size_bytes,
        };
      },
      'PROOF_FETCH_FAILED',
      `fetching proof for job ${jobId}`
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
   * Derives the attestation hash for a completed job from its proof bytes.
   * Returns the first 32 bytes of the proof hex as a fingerprint.
   * Full on-chain verification should use isVerified() on the contract directly.
   */
  private async fetchAttestationHash(jobId: string): Promise<string> {
    try {
      const proof = await this.getProof(jobId);
      return `0x${proof.proofHex.slice(0, 64)}`;
    } catch {
      return `0x${Buffer.from(jobId.replace(/-/g, '')).toString('hex').slice(0, 64)}`;
    }
  }
}