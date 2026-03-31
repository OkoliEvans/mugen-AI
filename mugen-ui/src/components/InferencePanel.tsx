import { useState, useRef } from 'react'
import { ElenxisClient, VerifyResult } from '@mugen/sdk'
import type { JobRecord } from '../hooks/useGateway'
import styles from './InferencePanel.module.css'

interface Props {
  onJobComplete: (job: JobRecord) => void
}

type Phase =
  | { kind: 'idle' }
  | { kind: 'submitting' }
  | { kind: 'polling'; jobId: string; startMs: number }
  | { kind: 'done'; result: VerifyResult }
  | { kind: 'error'; message: string }

const GATEWAY = import.meta.env.VITE_GATEWAY_URL ?? '/api'
const DEFAULT_INPUT = '[[0.1, 0.2, 0.3, 0.4]]'
const DEFAULT_MODEL = 'tiny_mlp_v1'

export default function InferencePanel({ onJobComplete }: Props) {
  const [modelId,    setModelId]    = useState(DEFAULT_MODEL)
  const [inputRaw,   setInputRaw]   = useState(DEFAULT_INPUT)
  const [inputError, setInputError] = useState<string | null>(null)
  const [phase,      setPhase]      = useState<Phase>({ kind: 'idle' })
  const clientRef = useRef<ElenxisClient | null>(null)

  function getClient() {
    if (!clientRef.current) {
      clientRef.current = new ElenxisClient({
        gatewayUrl:     GATEWAY,
        timeoutMs:      600_000,
        pollIntervalMs: 3_000,
      })
    }
    return clientRef.current
  }

  function parseInput(): number[][] | null {
    try {
      const parsed = JSON.parse(inputRaw)
      if (!Array.isArray(parsed) || !Array.isArray(parsed[0])) throw new Error()
      setInputError(null)
      return parsed as number[][]
    } catch {
      setInputError('Must be a 2D array — e.g. [[0.1, 0.2, 0.3, 0.4]]')
      return null
    }
  }

  async function handleSubmit() {
    const inputData = parseInput()
    if (!inputData) return

    setPhase({ kind: 'submitting' })
    const startMs = Date.now()

    try {
      const client = getClient()

      // 1. Submit once — get jobId immediately for display
      const jobId = await client.submitJob({ modelId, inputData })
      setPhase({ kind: 'polling', jobId, startMs })

      // 2. Poll until settled — no new job submitted
      const job = await client.waitForJob(jobId)

      // 3. Fetch attestation hash from proof
      const proof            = await client.getProof(jobId)
      const attestationHash  = `0x${proof.proofHex.slice(0, 64)}`

      const result: VerifyResult = {
        jobId,
        txHash:          job.txHash!,
        attestationHash,
        elapsedMs:       Date.now() - startMs,
      }

      setPhase({ kind: 'done', result })
      onJobComplete({
        jobId,
        txHash:          job.txHash!,
        attestationHash,
        elapsedMs:       result.elapsedMs,
        modelId,
        input:           inputData,
        settledAt:       new Date().toISOString(),
      })
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err)
      setPhase({ kind: 'error', message: msg })
    }
  }

  function reset() { setPhase({ kind: 'idle' }) }

  const busy = phase.kind === 'submitting' || phase.kind === 'polling'

  return (
    <div className={styles.panel}>
      <div className={styles.form}>
        <div className={styles.field}>
          <label className={styles.label}>Model ID</label>
          <input
            className={styles.input}
            value={modelId}
            onChange={e => setModelId(e.target.value)}
            disabled={busy}
            placeholder="tiny_mlp_v1"
            spellCheck={false}
          />
        </div>

        <div className={styles.field}>
          <label className={styles.label}>
            Input data <span className={styles.hint}>(2D array)</span>
          </label>
          <textarea
            className={`${styles.input} ${styles.textarea} ${inputError ? styles.inputErr : ''}`}
            value={inputRaw}
            onChange={e => { setInputRaw(e.target.value); setInputError(null) }}
            disabled={busy}
            rows={3}
            spellCheck={false}
          />
          {inputError && <span className={styles.errMsg}>{inputError}</span>}
        </div>

        <button
          className={styles.btn}
          onClick={handleSubmit}
          disabled={busy || phase.kind === 'done'}
        >
          {busy ? (
            <><span className={styles.spinner} /> Proving…</>
          ) : phase.kind === 'done' ? (
            '✓ Settled'
          ) : (
            'Run inference →'
          )}
        </button>
      </div>

      {phase.kind === 'submitting' && (
        <div className={styles.status}>
          <span className={styles.dot} /> Submitting job to gateway…
        </div>
      )}

      {phase.kind === 'polling' && (
        <div className={styles.status}>
          <span className={styles.dot} />
          Proving + settling on-chain
          <span className={styles.jobChip}>{phase.jobId.slice(0, 8)}…</span>
        </div>
      )}

      {phase.kind === 'error' && (
        <div className={styles.errorBox}>
          <div className={styles.errorTitle}>Failed</div>
          <div className={styles.errorMsg}>{phase.message}</div>
          <button className={styles.retryBtn} onClick={reset}>Try again</button>
        </div>
      )}

      {phase.kind === 'done' && (
        <div className={styles.result}>
          <div className={styles.resultRow}>
            <span className={styles.resultKey}>Job ID</span>
            <span className={styles.resultVal}>{phase.result.jobId}</span>
          </div>
          <div className={styles.resultRow}>
            <span className={styles.resultKey}>Tx hash</span>
            <a
              className={styles.resultLink}
              href={`https://sepolia.etherscan.io/tx/${phase.result.txHash}`}
              target="_blank"
              rel="noreferrer"
            >
              {phase.result.txHash.slice(0, 18)}…↗
            </a>
          </div>
          <div className={styles.resultRow}>
            <span className={styles.resultKey}>Attestation</span>
            <span className={styles.resultVal}>
              {phase.result.attestationHash.slice(0, 18)}…
            </span>
          </div>
          <div className={styles.resultRow}>
            <span className={styles.resultKey}>Elapsed</span>
            <span className={styles.resultVal}>
              {(phase.result.elapsedMs / 1000).toFixed(1)}s
            </span>
          </div>
          <button className={styles.retryBtn} onClick={reset}>Run another →</button>
        </div>
      )}
    </div>
  )
}