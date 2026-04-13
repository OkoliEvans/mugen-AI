// src/http.ts
import axios, { AxiosError } from "axios";

// src/errors.ts
var VeilError = class extends Error {
  code;
  cause;
  constructor(code, message, cause) {
    super(message);
    this.name = "VeilError";
    this.code = code;
    this.cause = cause;
    Object.setPrototypeOf(this, new.target.prototype);
  }
};

// src/http.ts
var DEFAULT_MAX_RETRIES = 3;
var RETRY_DELAY_MS = 500;
function createHttpClient(baseURL, maxRetries = DEFAULT_MAX_RETRIES) {
  const client = axios.create({
    baseURL,
    headers: { "Content-Type": "application/json" },
    timeout: 3e4
  });
  client.interceptors.response.use(
    (response) => response,
    async (error) => {
      const config = error.config;
      if (!config) return Promise.reject(error);
      config._retryCount = config._retryCount ?? 0;
      const isRetryable = !error.response || error.response.status >= 500 && error.response.status < 600;
      if (isRetryable && config._retryCount < maxRetries) {
        config._retryCount += 1;
        const delay = RETRY_DELAY_MS * Math.pow(2, config._retryCount - 1);
        await sleep(delay);
        return client(config);
      }
      return Promise.reject(error);
    }
  );
  return client;
}
async function safeRequest(fn, errorCode, context) {
  try {
    return await fn();
  } catch (err) {
    const message = err instanceof AxiosError ? `${context}: ${err.response?.data?.error ?? err.message}` : `${context}: ${String(err)}`;
    throw new VeilError(errorCode, message, err);
  }
}
function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// src/poller.ts
var TERMINAL_STATES = /* @__PURE__ */ new Set(["done", "settled", "failed"]);
function mapJob(raw) {
  return {
    jobId: raw.job_id,
    status: raw.status,
    proofPath: raw.proof_path,
    txHash: raw.tx_hash,
    reason: raw.reason
  };
}
async function pollUntilDone(client, jobId, timeoutMs, intervalMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const job = await safeRequest(
      async () => {
        const { data } = await client.get(`/v1/jobs/${jobId}`);
        return mapJob(data);
      },
      "POLL_FAILED",
      `polling job ${jobId}`
    );
    if (TERMINAL_STATES.has(job.status)) {
      if (job.status === "failed") {
        throw new VeilError(
          "JOB_FAILED",
          `job ${jobId} failed: ${job.reason ?? "unknown reason"}`
        );
      }
      if (job.status === "done" && !job.txHash) {
        await sleep2(intervalMs);
        continue;
      }
      return job;
    }
    await sleep2(intervalMs);
  }
  throw new VeilError(
    "TIMEOUT",
    `job ${jobId} did not complete within ${timeoutMs}ms`
  );
}
function sleep2(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// src/client.ts
var DEFAULT_TIMEOUT_MS = 3e5;
var DEFAULT_POLL_INTERVAL = 4e3;
var DEFAULT_MAX_RETRIES2 = 3;
function mapJob2(raw) {
  return {
    jobId: raw.job_id,
    status: raw.status,
    proofPath: raw.proof_path,
    txHash: raw.tx_hash,
    attestationHash: raw.attestation_hash,
    reason: raw.reason
  };
}
var VeilClient = class {
  http;
  timeoutMs;
  pollIntervalMs;
  constructor(config) {
    if (!config.gatewayUrl) {
      throw new VeilError("NETWORK_ERROR", "gatewayUrl is required");
    }
    this.http = createHttpClient(config.gatewayUrl, config.maxRetries ?? DEFAULT_MAX_RETRIES2);
    this.timeoutMs = config.timeoutMs ?? DEFAULT_TIMEOUT_MS;
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
  async verifyInference(params) {
    const startMs = Date.now();
    const jobId = await this.submitJob(params);
    const job = await pollUntilDone(
      this.http,
      jobId,
      this.timeoutMs,
      this.pollIntervalMs
    );
    const attestationHash = await this.fetchAttestationHash(jobId);
    return {
      jobId,
      status: job.status === "done" ? "verified" : job.status,
      attestationHash,
      txHash: job.txHash,
      // undefined until batch settles — this is expected
      elapsedMs: Date.now() - startMs
    };
  }
  /**
   * Submit a job without waiting for completion.
   * Use this for fire-and-forget flows where you'll poll manually.
   *
   * @returns The jobId to track with waitForJob() or getJob()
   */
  async submitJob(params) {
    return safeRequest(
      async () => {
        const { data } = await this.http.post("/v1/jobs", {
          model_id: params.modelId,
          input_data: params.inputData
        });
        return data.job_id;
      },
      "SUBMIT_FAILED",
      "submitting inference job"
    );
  }
  /**
   * Get the current status of a job.
   */
  async getJob(jobId) {
    return safeRequest(
      async () => {
        const { data } = await this.http.get(`/v1/jobs/${jobId}`);
        return mapJob2(data);
      },
      "POLL_FAILED",
      `fetching job ${jobId}`
    );
  }
  /**
   * Wait for an existing job to complete.
   * Useful when combined with submitJob() for manual control.
   */
  async waitForJob(jobId) {
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
  async getProof(jobId) {
    return safeRequest(
      async () => {
        const { data } = await this.http.get(`/v1/jobs/${jobId}/proof`);
        return {
          jobId: data.job_id,
          // proofHex repurposed — carries attestation_hash for this endpoint.
          // Full Groth16 bytes are aggregated per batch, not per job.
          proofHex: data.attestation_hash ?? "",
          sizeBytes: 0
        };
      },
      "PROOF_FETCH_FAILED",
      `fetching proof metadata for job ${jobId}`
    );
  }
  /**
   * Check gateway liveness.
   * @returns true if the gateway is reachable and healthy
   */
  async healthCheck() {
    try {
      const { data } = await this.http.get("/healthz");
      return data.status === "ok";
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
  async fetchAttestationHash(jobId) {
    try {
      const job = await this.getJob(jobId);
      if (job.attestationHash) return job.attestationHash;
      const proof = await this.getProof(jobId);
      return proof.proofHex || "";
    } catch {
      return "";
    }
  }
};
export {
  VeilClient,
  VeilError
};
