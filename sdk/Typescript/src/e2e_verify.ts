/// <reference types="node" />
/**
 * Veil SDK — Live E2E test for verifyInference
 *
 * Usage:
 *   GATEWAY_URL=http://localhost:8080 npx tsx e2e_verify.ts
 *
 * Optional overrides:
 *   TIMEOUT_MS=180000    — poll timeout in ms (default: 120000)
 *   MODEL_ID=tiny_mlp_v1
 */

import { VeilClient } from './client';

const GATEWAY_URL = process.env.GATEWAY_URL  ?? 'http://localhost:8080';
const TIMEOUT_MS  = Number(process.env.TIMEOUT_MS ?? '300000');
const MODEL_ID    = process.env.MODEL_ID     ?? 'tiny_mlp_v1';

// Shape [1, 4] — matches tiny_mlp expected input
const INPUT_DATA: number[][] = [
  [0.2, 0.5, 0.4, 0.81],
];

// ── Helpers ──────────────────────────────────────────────────────────────────

function ts() { return new Date().toISOString(); }
function log(label: string, value?: unknown) {
  value !== undefined
    ? console.log(`[${ts()}] ${label}:`, value)
    : console.log(`[${ts()}] ${label}`);
}
function pass(msg: string)  { console.log(`  ✅  PASS — ${msg}`); }
function fail(msg: string)  { console.error(`  ❌  FAIL — ${msg}`); }
function section(title: string) {
  console.log(`\n── ${title} ${'─'.repeat(Math.max(0, 50 - title.length - 4))}`);
}

// ── Main ─────────────────────────────────────────────────────────────────────

async function main() {
  console.log('\n═══════════════════════════════════════════════');
  console.log('  Veil SDK — Live E2E: verifyInference');
  console.log('═══════════════════════════════════════════════\n');

  log('Gateway', GATEWAY_URL);
  log('Model  ', MODEL_ID);
  log('Input  ', JSON.stringify(INPUT_DATA));
  log('Timeout', `${TIMEOUT_MS}ms`);

  const client = new VeilClient({
    gatewayUrl:     GATEWAY_URL,
    timeoutMs:      TIMEOUT_MS,
    pollIntervalMs: 4_000,
    maxRetries:     3,
  });

  let allPassed = true;

  function assert(label: string, condition: boolean, got: unknown) {
    if (condition) {
      pass(label);
    } else {
      fail(`${label} — got: ${JSON.stringify(got)}`);
      allPassed = false;
    }
  }

  // ── Step 0: health ────────────────────────────────────────────────────────
  section('Step 0 — Health check');

  const health = await client.healthCheck();
  if (!health) {
    fail('Gateway did not respond to /healthz — is it running?');
    process.exit(1);
  }
  pass('Gateway is healthy');

  // ── Step 1: verify_inference ──────────────────────────────────────────────
  section('Step 1 — Submit + wait (verifyInference)');

  let result: Awaited<ReturnType<typeof client.verifyInference>>;
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

  assert(
    'result.jobId is a non-empty string',
    typeof result.jobId === 'string' && result.jobId.length > 0,
    result.jobId,
  );
  assert(
    'result.elapsedMs is a positive number',
    typeof result.elapsedMs === 'number' && result.elapsedMs > 0,
    result.elapsedMs,
  );

  // tx_hash only guaranteed when settled
  if (result.txHash) {
    assert(
      'result.txHash starts with 0x',
      result.txHash.startsWith('0x'),
      result.txHash,
    );
  } else {
    log('ℹ️  result.txHash', 'absent (job done but not yet settled)');
  }

  // attestation_hash — present once gateway exposes it (impl guide §6c)
  if (result.attestationHash) {
    assert(
      'result.attestationHash is 0x + 32 bytes (66 chars)',
      result.attestationHash.startsWith('0x') && result.attestationHash.length === 66,
      result.attestationHash,
    );
  } else {
    log('ℹ️  attestationHash', 'not yet exposed by gateway — add field to JobStatusResponse');
  }

  // ── Step 2: cross-check via getJob ────────────────────────────────────────
  section(`Step 2 — Cross-check via getJob(${result.jobId})`);

  let job: Awaited<ReturnType<typeof client.getJob>> | undefined;
  try {
    job = await client.getJob(result.jobId);
  } catch (err: unknown) {
    fail(`getJob threw — ${(err as Error).message}`);
    allPassed = false;
  }

  if (job) {
    assert('job.jobId matches',           job.jobId === result.jobId, job.jobId);
    assert('job.status is terminal',      ['done','settled','failed'].includes(job.status), job.status);
    if (result.txHash) {
      assert('job.txHash matches result', job.txHash === result.txHash, job.txHash);
    }
  }

  // ── Step 3: fetch raw proof ───────────────────────────────────────────────
  section('Step 3 — Fetch proof bytes via getProof');

  let proof: Awaited<ReturnType<typeof client.getProof>> | undefined;
  try {
    proof = await client.getProof(result.jobId);
  } catch (err: unknown) {
    fail(`getProof threw — ${(err as Error).message}`);
    allPassed = false;
  }

  if (proof) {
    assert(
      'proof.proofHex is a non-empty hex string (≥64 chars)',
      typeof proof.proofHex === 'string' && proof.proofHex.length >= 64,
      `${proof.proofHex.slice(0, 16)}…`,
    );
    assert(
      'proof.sizeBytes > 0',
      typeof proof.sizeBytes === 'number' && proof.sizeBytes > 0,
      proof.sizeBytes,
    );
  }

  // ── Summary ───────────────────────────────────────────────────────────────
  console.log('\n═══════════════════════════════════════════════');
  console.log('  Result Summary');
  console.log('═══════════════════════════════════════════════');
  console.log('  Job ID           :', result.jobId);
  console.log('  Status           :', result.status ?? 'n/a');
  console.log('  Tx Hash          :', result.txHash ?? 'n/a');
  console.log('  Attestation Hash :', result.attestationHash ?? 'n/a (gateway field pending)');
  console.log('  Proof Size       :', proof ? `${proof.sizeBytes} bytes` : 'n/a');
  console.log('  Elapsed (SDK)    :', `${result.elapsedMs}ms`);
  console.log('═══════════════════════════════════════════════');

  if (allPassed) {
    console.log('\n  🎉  All assertions passed\n');
    process.exit(0);
  } else {
    console.error('\n  💥  Some assertions failed — see above\n');
    process.exit(1);
  }
}

main();