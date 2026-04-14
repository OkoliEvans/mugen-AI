// ── Job lifecycle ─────────────────────────────────────────────────────────────

export type JobStatus = "queued" | "running" | "done" | "settled" | "failed";

export interface Job {
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

// ── Attestation ───────────────────────────────────────────────────────────────

export interface Attestation {
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

// ── verifyInference result ────────────────────────────────────────────────────

export interface VerifyResult {
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

// ── Proof data ────────────────────────────────────────────────────────────────

export interface ProofData {
  jobId: string;
  /** Full proof as a hex string */
  proofHex: string;
  /** Proof size in bytes */
  sizeBytes: number;
}

// ── Config ────────────────────────────────────────────────────────────────────

export interface VeilConfig {
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
  /**
   * Wallet address for VeilVault fee deduction.
   * Required when the gateway has VAULT_ADDRESS configured.
   * Can be overridden per-call in submitJob/verifyInference params.
   */
  walletAddress?: string;
}

// ── Errors ────────────────────────────────────────────────────────────────────

export type VeilErrorCode =
  | "SUBMIT_FAILED"
  | "POLL_FAILED"
  | "JOB_FAILED"
  | "TIMEOUT"
  | "PROOF_FETCH_FAILED"
  | "NETWORK_ERROR";
