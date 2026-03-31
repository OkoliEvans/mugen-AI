/// <reference types="node" />
/**
 * Elenxis SDK — Live E2E test for verifyInference
 *
 * Usage:
 *   GATEWAY_URL=http://localhost:8080 npx tsx e2e_verify.ts
 *
 * Or with a custom timeout (ms):
 *   GATEWAY_URL=http://localhost:8080 TIMEOUT_MS=180000 npx tsx e2e_verify.ts
 */

import { ElenxisClient } from './client'; // adjust if your entry point differs

const GATEWAY_URL  = process.env.GATEWAY_URL  ?? 'http://localhost:8080';
const TIMEOUT_MS   = Number(process.env.TIMEOUT_MS ?? '120000');
const MODEL_ID     = 'tiny_mlp_v1';

// ── Sample input ─────────────────────────────────────────────────────────────
// Shape: [1, 4] — adjust to match your model's actual input dimensions
const INPUT_DATA: number[][] = [
  [0.1, 0.2, 0.3, 0.4],
];

// ── Helpers ──────────────────────────────────────────────────────────────────
function log(label: string, value?: unknown) {
  const ts = new Date().toISOString();
  if (value !== undefined) {
    console.log(`[${ts}] ${label}:`, value);
  } else {
    console.log(`[${ts}] ${label}`);
  }
}

function pass(msg: string) { console.log(`\n  ✅ PASS — ${msg}`); }
function fail(msg: string) { console.error(`\n  ❌ FAIL — ${msg}`); }

// ── Main ─────────────────────────────────────────────────────────────────────
async function main() {
  console.log('\n═══════════════════════════════════════════════');
  console.log('  Elenxis SDK — Live E2E: verifyInference');
  console.log('═══════════════════════════════════════════════\n');

  log('Gateway', GATEWAY_URL);
  log('Model  ', MODEL_ID);
  log('Input  ', JSON.stringify(INPUT_DATA));
  log('Timeout', `${TIMEOUT_MS}ms`);
  console.log();

  const client = new ElenxisClient({
    gatewayUrl:     GATEWAY_URL,
    timeoutMs:      TIMEOUT_MS,
    pollIntervalMs: 4_000,   // poll every 4 s — tune down if your prover is fast
    maxRetries:     3,
  });

  // ── Step 0: health check ─────────────────────────────────────────────────
  log('Step 0', 'Gateway health check');
  const healthy = await client.healthCheck();
  if (!healthy) {
    fail('Gateway did not respond to /healthz — is it running?');
    process.exit(1);
  }
  pass('Gateway is healthy');

  // ── Step 1: verifyInference ──────────────────────────────────────────────
  log('\nStep 1', 'Submitting inference + waiting for on-chain settlement');
  const t0 = Date.now();

  let result;
  try {
    result = await client.verifyInference({
      modelId:   MODEL_ID,
      inputData: INPUT_DATA,
    });
  } catch (err: unknown) {
    fail(`verifyInference threw — ${(err as Error).message}`);
    console.error(err);
    process.exit(1);
  }

  const elapsed = Date.now() - t0;

  // ── Assertions ───────────────────────────────────────────────────────────
  let allPassed = true;

  function assert(label: string, condition: boolean, got: unknown) {
    if (condition) {
      pass(label);
    } else {
      fail(`${label} — got: ${JSON.stringify(got)}`);
      allPassed = false;
    }
  }

  console.log('\n── Assertions ──────────────────────────────────\n');

  assert(
    'result.jobId is a non-empty string',
    typeof result.jobId === 'string' && result.jobId.length > 0,
    result.jobId
  );

  assert(
    'result.txHash starts with 0x',
    typeof result.txHash === 'string' && result.txHash.startsWith('0x'),
    result.txHash
  );

  assert(
    'result.attestationHash starts with 0x and is 66 chars',
    typeof result.attestationHash === 'string' &&
    result.attestationHash.startsWith('0x') &&
    result.attestationHash.length === 66,
    result.attestationHash
  );

  assert(
    'result.elapsedMs is a positive number',
    typeof result.elapsedMs === 'number' && result.elapsedMs > 0,
    result.elapsedMs
  );

  // ── Step 2: cross-check with getJob ─────────────────────────────────────
  log('\nStep 2', `Cross-checking job status via getJob(${result.jobId})`);
  let job;
  try {
    job = await client.getJob(result.jobId);
  } catch (err: unknown) {
    fail(`getJob threw — ${(err as Error).message}`);
    allPassed = false;
  }

  if (job) {
    assert(
      'job.status === "settled"',
      job.status === 'settled',
      job.status
    );

    assert(
      'job.txHash matches verifyInference result',
      job.txHash === result.txHash,
      job.txHash
    );
  }

  // ── Step 3: fetch raw proof ──────────────────────────────────────────────
  log('\nStep 3', 'Fetching raw proof bytes via getProof()');
  let proof;
  try {
    proof = await client.getProof(result.jobId);
  } catch (err: unknown) {
    fail(`getProof threw — ${(err as Error).message}`);
    allPassed = false;
  }

  if (proof) {
    assert(
      'proof.proofHex is a non-empty hex string',
      typeof proof.proofHex === 'string' && proof.proofHex.length >= 64,
      `${proof.proofHex.slice(0, 16)}...`
    );

    assert(
      'proof.sizeBytes > 0',
      typeof proof.sizeBytes === 'number' && proof.sizeBytes > 0,
      proof.sizeBytes
    );
  }

  // ── Summary ───────────────────────────────────────────────────────────────
  console.log('\n═══════════════════════════════════════════════');
  console.log('  Result Summary');
  console.log('═══════════════════════════════════════════════');
  console.log('  Job ID          :', result.jobId);
  console.log('  Tx Hash         :', result.txHash);
  console.log('  Attestation Hash:', result.attestationHash);
  console.log('  Proof size      :', proof ? `${proof.sizeBytes} bytes` : 'n/a');
  console.log('  Elapsed (SDK)   :', `${result.elapsedMs}ms`);
  console.log('  Elapsed (wall)  :', `${elapsed}ms`);
  console.log('═══════════════════════════════════════════════');

  if (allPassed) {
    console.log('\n  🎉 All assertions passed\n');
    process.exit(0);
  } else {
    console.error('\n  💥 Some assertions failed — see above\n');
    process.exit(1);
  }
}

main();