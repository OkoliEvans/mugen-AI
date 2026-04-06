// sdk/Typescript/src/client.test.ts

import axios from "axios";
import { VeilClient } from "./client";
import { VeilError } from "./errors";

jest.mock("axios");

const mockedAxios = axios as jest.Mocked<typeof axios>;

const mockGet  = jest.fn();
const mockPost = jest.fn();

mockedAxios.create.mockReturnValue({
  get:  mockGet,
  post: mockPost,
  interceptors: {
    response: { use: jest.fn() },
    request:  { use: jest.fn() },
  },
} as unknown as ReturnType<typeof axios.create>);

const GATEWAY_URL = "http://localhost:8080";
const JOB_ID      = "test-job-uuid-1234";
const TX_HASH     = "0xabc123def456";
// 64-char hex string for proof_hex so fetchAttestationHash returns a valid 0x+64 string
const PROOF_HEX   = "deadbeef".repeat(16); // 64 chars

function makeClient(overrides = {}) {
  return new VeilClient({
    gatewayUrl:     GATEWAY_URL,
    timeoutMs:      5_000,
    pollIntervalMs: 100,
    ...overrides,
  });
}

beforeEach(() => {
  jest.clearAllMocks();
});

// ── Constructor ───────────────────────────────────────────────────────────────

describe("VeilClient constructor", () => {
  it("throws if gatewayUrl is missing", () => {
    expect(() => new VeilClient({ gatewayUrl: "" })).toThrow(VeilError);
  });

  it("constructs successfully with valid config", () => {
    expect(() => makeClient()).not.toThrow();
  });
});

// ── healthCheck ───────────────────────────────────────────────────────────────

describe("healthCheck", () => {
  it("returns true when gateway responds ok", async () => {
    mockGet.mockResolvedValueOnce({ data: { status: "ok" } });
    expect(await makeClient().healthCheck()).toBe(true);
  });

  it("returns false when gateway is unreachable", async () => {
    mockGet.mockRejectedValueOnce(new Error("ECONNREFUSED"));
    expect(await makeClient().healthCheck()).toBe(false);
  });
});

// ── submitJob ─────────────────────────────────────────────────────────────────

describe("submitJob", () => {
  it("returns jobId on success", async () => {
    mockPost.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "queued" },
    });
    const jobId = await makeClient().submitJob({
      modelId:   "tiny_mlp_v1",
      inputData: [[0.1, 0.2]],
    });
    expect(jobId).toBe(JOB_ID);
  });

  it("throws VeilError on network failure", async () => {
    mockPost.mockRejectedValueOnce(new Error("network error"));
    await expect(
      makeClient().submitJob({ modelId: "tiny_mlp_v1", inputData: [[0.1]] })
    ).rejects.toThrow(VeilError);
  });
});

// ── getJob ────────────────────────────────────────────────────────────────────

describe("getJob", () => {
  it("returns job status", async () => {
    mockGet.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "running" },
    });
    const job = await makeClient().getJob(JOB_ID);
    expect(job.status).toBe("running");
  });
});

// ── verifyInference ───────────────────────────────────────────────────────────

describe("verifyInference", () => {
  it("completes end-to-end and returns VerifyResult", async () => {
    // 1. submitJob POST
    mockPost.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "queued" },
    });

    // 2. poll: running → done with tx_hash
    // Client throws if txHash is absent on terminal state, so include it here
    mockGet
      .mockResolvedValueOnce({
        data: { job_id: JOB_ID, status: "running" },
      })
      .mockResolvedValueOnce({
        data: { job_id: JOB_ID, status: "done", tx_hash: TX_HASH },
      })
      // 3. fetchAttestationHash → getProof
      .mockResolvedValueOnce({
        data: { job_id: JOB_ID, proof_hex: PROOF_HEX, size_bytes: 64 },
      });

    const result = await makeClient().verifyInference({
      modelId:   "tiny_mlp_v1",
      inputData: [[0.1, 0.2, 0.3, 0.4]],
    });

    expect(result.jobId).toBe(JOB_ID);
    expect(result.txHash).toBe(TX_HASH);
    // attestationHash = "0x" + proofHex.slice(0, 64) — always starts with 0x
    expect(result.attestationHash).toMatch(/^0x[0-9a-f]{64}$/i);
    expect(result.elapsedMs).toBeGreaterThanOrEqual(0);
  });

  it("throws VeilError with code JOB_FAILED when job fails", async () => {
    mockPost.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "queued" },
    });
    mockGet.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "failed", reason: "proof generation failed" },
    });

    await expect(
      makeClient().verifyInference({ modelId: "tiny_mlp_v1", inputData: [[0.1]] })
    ).rejects.toMatchObject({ code: "JOB_FAILED" });
  });

  it("throws VeilError with code JOB_FAILED when job done but no tx_hash", async () => {
    // Client throws JOB_FAILED when txHash is absent — matches client.ts line:
    // if (!job.txHash) throw new VeilError('JOB_FAILED', ...)
    mockPost.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "queued" },
    });
    mockGet.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "done" }, // no tx_hash
    });

    await expect(
      makeClient().verifyInference({ modelId: "tiny_mlp_v1", inputData: [[0.1]] })
    ).rejects.toMatchObject({ code: "JOB_FAILED" });
  });

  it("throws VeilError with code TIMEOUT when job takes too long", async () => {
    mockPost.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "queued" },
    });
    // Always return running so poller times out
    mockGet.mockResolvedValue({
      data: { job_id: JOB_ID, status: "running" },
    });

    await expect(
      makeClient({ timeoutMs: 300, pollIntervalMs: 50 }).verifyInference({
        modelId:   "tiny_mlp_v1",
        inputData: [[0.1]],
      })
    ).rejects.toMatchObject({ code: "TIMEOUT" });
  });
});

// ── getProof ──────────────────────────────────────────────────────────────────

describe("getProof", () => {
  it("returns proof data", async () => {
    mockGet.mockResolvedValueOnce({
      data: { job_id: JOB_ID, proof_hex: "deadbeef", size_bytes: 4 },
    });
    const proof = await makeClient().getProof(JOB_ID);
    expect(proof.proofHex).toBe("deadbeef");
    expect(proof.sizeBytes).toBe(4);
  });

  it("throws VeilError on failure", async () => {
    mockGet.mockRejectedValueOnce(new Error("not found"));
    await expect(makeClient().getProof(JOB_ID)).rejects.toThrow(VeilError);
  });
});

// ── waitForJob ────────────────────────────────────────────────────────────────

describe("waitForJob", () => {
  it("returns job when it reaches terminal state", async () => {
    mockGet
      .mockResolvedValueOnce({ data: { job_id: JOB_ID, status: "running" } })
      .mockResolvedValueOnce({ data: { job_id: JOB_ID, status: "done", tx_hash: TX_HASH } });

    const job = await makeClient().waitForJob(JOB_ID);
    expect(job.status).toBe("done");
    expect(job.txHash).toBe(TX_HASH);
  });
});