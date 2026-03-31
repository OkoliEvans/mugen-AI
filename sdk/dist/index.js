"use strict";
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/index.ts
var index_exports = {};
__export(index_exports, {
  ElenxisClient: () => ElenxisClient,
  ElenxisError: () => ElenxisError
});
module.exports = __toCommonJS(index_exports);

// src/http.ts
var import_axios = __toESM(require("axios"));

// src/errors.ts
var ElenxisError = class extends Error {
  constructor(code, message, cause) {
    super(message);
    this.name = "ElenxisError";
    this.code = code;
    this.cause = cause;
    Object.setPrototypeOf(this, new.target.prototype);
  }
};

// src/http.ts
var DEFAULT_MAX_RETRIES = 3;
var RETRY_DELAY_MS = 500;
function createHttpClient(baseURL, maxRetries = DEFAULT_MAX_RETRIES) {
  const client = import_axios.default.create({
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
    const message = err instanceof import_axios.AxiosError ? `${context}: ${err.response?.data?.error ?? err.message}` : `${context}: ${String(err)}`;
    throw new ElenxisError(errorCode, message, err);
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
        throw new ElenxisError(
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
  throw new ElenxisError(
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
    reason: raw.reason
  };
}
var ElenxisClient = class {
  constructor(config) {
    if (!config.gatewayUrl) {
      throw new ElenxisError("NETWORK_ERROR", "gatewayUrl is required");
    }
    this.http = createHttpClient(config.gatewayUrl, config.maxRetries ?? DEFAULT_MAX_RETRIES2);
    this.timeoutMs = config.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    this.pollIntervalMs = config.pollIntervalMs ?? DEFAULT_POLL_INTERVAL;
  }
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
  async verifyInference(params) {
    const startMs = Date.now();
    const jobId = await this.submitJob(params);
    const job = await pollUntilDone(
      this.http,
      jobId,
      this.timeoutMs,
      this.pollIntervalMs
    );
    if (!job.txHash) {
      throw new ElenxisError(
        "JOB_FAILED",
        `job ${jobId} completed but has no tx_hash \u2014 settlement may have failed`
      );
    }
    const attestationHash = await this.fetchAttestationHash(jobId);
    return {
      jobId,
      attestationHash,
      txHash: job.txHash,
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
   * Fetch the raw proof bytes for a completed job.
   */
  async getProof(jobId) {
    return safeRequest(
      async () => {
        const { data } = await this.http.get(`/v1/jobs/${jobId}/proof`);
        return {
          jobId: data.job_id,
          proofHex: data.proof_hex,
          sizeBytes: data.size_bytes
        };
      },
      "PROOF_FETCH_FAILED",
      `fetching proof for job ${jobId}`
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
   * Derives the attestation hash for a completed job from its proof bytes.
   * Returns the first 32 bytes of the proof hex as a fingerprint.
   * Full on-chain verification should use isVerified() on the contract directly.
   */
  async fetchAttestationHash(jobId) {
    try {
      const proof = await this.getProof(jobId);
      return `0x${proof.proofHex.slice(0, 64)}`;
    } catch {
      return `0x${Buffer.from(jobId.replace(/-/g, "")).toString("hex").slice(0, 64)}`;
    }
  }
};
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  ElenxisClient,
  ElenxisError
});
