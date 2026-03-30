import axios from "axios";
import { ElenxisClient } from "../src/client";
import { ElenxisError } from "../src/errors";

jest.mock("axios");

const mockedAxios = axios as jest.Mocked<typeof axios>;

// Mock axios.create to return a mock instance
const mockGet = jest.fn();
const mockPost = jest.fn();

mockedAxios.create.mockReturnValue({
  get: mockGet,
  post: mockPost,
  interceptors: {
    response: { use: jest.fn() },
    request: { use: jest.fn() },
  },
} as unknown as ReturnType<typeof axios.create>);

const GATEWAY_URL = "http://localhost:8080";
const JOB_ID = "test-job-uuid-1234";
const TX_HASH = "0xabc123def456";

function makeClient(overrides = {}) {
  return new ElenxisClient({
    gatewayUrl: GATEWAY_URL,
    timeoutMs: 5_000,
    pollIntervalMs: 100,
    ...overrides,
  });
}

beforeEach(() => {
  jest.clearAllMocks();
});

// ── Constructor ───────────────────────────────────────────────────────────────

describe("ElenxisClient constructor", () => {
  it("throws if gatewayUrl is missing", () => {
    expect(() => new ElenxisClient({ gatewayUrl: "" })).toThrow(ElenxisError);
  });

  it("constructs successfully with valid config", () => {
    expect(() => makeClient()).not.toThrow();
  });
});

// ── healthCheck ───────────────────────────────────────────────────────────────

describe("healthCheck", () => {
  it("returns true when gateway responds ok", async () => {
    mockGet.mockResolvedValueOnce({ data: { status: "ok" } });
    const client = makeClient();
    expect(await client.healthCheck()).toBe(true);
  });

  it("returns false when gateway is unreachable", async () => {
    mockGet.mockRejectedValueOnce(new Error("ECONNREFUSED"));
    const client = makeClient();
    expect(await client.healthCheck()).toBe(false);
  });
});

// ── submitJob ─────────────────────────────────────────────────────────────────

describe("submitJob", () => {
  it("returns jobId on success", async () => {
    mockPost.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "queued" },
    });
    const client = makeClient();
    const jobId = await client.submitJob({
      modelId: "tiny_mlp_v1",
      inputData: [[0.1, 0.2]],
    });
    expect(jobId).toBe(JOB_ID);
  });

  it("throws ElenxisError on network failure", async () => {
    mockPost.mockRejectedValueOnce(new Error("network error"));
    const client = makeClient();
    await expect(
      client.submitJob({ modelId: "tiny_mlp_v1", inputData: [[0.1]] }),
    ).rejects.toThrow(ElenxisError);
  });
});

// ── getJob ────────────────────────────────────────────────────────────────────

describe("getJob", () => {
  it("returns job status", async () => {
    mockGet.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "running" },
    });
    const client = makeClient();
    const job = await client.getJob(JOB_ID);
    expect(job.status).toBe("running");
  });
});

// ── verifyInference ───────────────────────────────────────────────────────────

describe("verifyInference", () => {
  it("completes end-to-end and returns VerifyResult", async () => {
    // submit
    mockPost.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "queued" },
    });
    // poll x2: running, then done with txHash
    mockGet
      .mockResolvedValueOnce({ data: { job_id: JOB_ID, status: "running" } })
      .mockResolvedValueOnce({
        data: { job_id: JOB_ID, status: "done", tx_hash: TX_HASH },
      })
      // proof fetch
      .mockResolvedValueOnce({
        data: {
          job_id: JOB_ID,
          proof_hex: "deadbeef".repeat(16),
          size_bytes: 64,
        },
      });

    const client = makeClient();
    const result = await client.verifyInference({
      modelId: "tiny_mlp_v1",
      inputData: [[0.1, 0.2, 0.3, 0.4]],
    });

    expect(result.jobId).toBe(JOB_ID);
    expect(result.txHash).toBe(TX_HASH);
    expect(result.attestationHash).toMatch(/^0x/);
    expect(result.elapsedMs).toBeGreaterThanOrEqual(0);
  });

  it("throws ElenxisError with code JOB_FAILED when job fails", async () => {
    mockPost.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "queued" },
    });
    mockGet.mockResolvedValueOnce({
      data: {
        job_id: JOB_ID,
        status: "failed",
        reason: "proof generation failed",
      },
    });

    const client = makeClient();
    await expect(
      client.verifyInference({ modelId: "tiny_mlp_v1", inputData: [[0.1]] }),
    ).rejects.toMatchObject({ code: "JOB_FAILED" });
  });

  it("throws ElenxisError with code TIMEOUT when job takes too long", async () => {
    mockPost.mockResolvedValueOnce({
      data: { job_id: JOB_ID, status: "queued" },
    });
    // Always return running
    mockGet.mockResolvedValue({ data: { job_id: JOB_ID, status: "running" } });

    const client = makeClient({ timeoutMs: 300, pollIntervalMs: 50 });
    await expect(
      client.verifyInference({ modelId: "tiny_mlp_v1", inputData: [[0.1]] }),
    ).rejects.toMatchObject({ code: "TIMEOUT" });
  });
});

// ── getProof ──────────────────────────────────────────────────────────────────

describe("getProof", () => {
  it("returns proof data", async () => {
    mockGet.mockResolvedValueOnce({
      data: { job_id: JOB_ID, proof_hex: "deadbeef", size_bytes: 4 },
    });
    const client = makeClient();
    const proof = await client.getProof(JOB_ID);
    expect(proof.proofHex).toBe("deadbeef");
    expect(proof.sizeBytes).toBe(4);
  });

  it("throws ElenxisError on failure", async () => {
    mockGet.mockRejectedValueOnce(new Error("not found"));
    const client = makeClient();
    await expect(client.getProof(JOB_ID)).rejects.toThrow(ElenxisError);
  });
});
