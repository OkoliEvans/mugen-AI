import { useState } from 'react'
import type { JobRecord } from '../hooks/useGateway'
import styles from './JobHistory.module.css'

interface Props {
  jobs:         JobRecord[]
  onClear:      () => void
}

function elapsed(ms: number) {
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  return `${(ms / 60_000).toFixed(1)}m`
}

function timeAgo(iso: string) {
  const diff = Date.now() - new Date(iso).getTime()
  if (diff < 60_000)  return 'just now'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`
  return `${Math.floor(diff / 3_600_000)}h ago`
}

export default function JobHistory({ jobs, onClear }: Props) {
  const [open,     setOpen]     = useState(false)
  const [expanded, setExpanded] = useState<string | null>(null)

  if (jobs.length === 0) return null

  return (
    <div className={styles.wrap}>
      <button className={styles.toggle} onClick={() => setOpen(o => !o)}>
        <span className={styles.toggleLabel}>
          Job history
          <span className={styles.count}>{jobs.length}</span>
        </span>
        <span className={styles.chevron} style={{ transform: open ? 'rotate(180deg)' : 'none' }}>
          ↓
        </span>
      </button>

      {open && (
        <div className={styles.list}>
          <div className={styles.listHeader}>
            <span className={styles.listHint}>Most recent first · stored locally</span>
            <button className={styles.clearBtn} onClick={onClear}>Clear</button>
          </div>

          {jobs.map(job => (
            <div
              key={job.jobId}
              className={styles.row}
              onClick={() => setExpanded(expanded === job.jobId ? null : job.jobId)}
            >
              <div className={styles.rowMain}>
                <span className={styles.rowId}>{job.jobId.slice(0, 8)}…</span>
                <span className={styles.rowModel}>{job.modelId}</span>
                <span className={styles.rowElapsed}>{elapsed(job.elapsedMs)}</span>
                <span className={styles.rowTime}>{timeAgo(job.settledAt)}</span>
                <span className={`${styles.badge} ${styles.settled}`}>settled</span>
              </div>

              {expanded === job.jobId && (
                <div className={styles.detail}>
                  <div className={styles.detailRow}>
                    <span className={styles.dk}>Job ID</span>
                    <span className={styles.dv}>{job.jobId}</span>
                  </div>
                  <div className={styles.detailRow}>
                    <span className={styles.dk}>Tx hash</span>
                    <a
                      className={styles.dvLink}
                      href={`https://sepolia.etherscan.io/tx/${job.txHash}`}
                      target="_blank"
                      rel="noreferrer"
                      onClick={e => e.stopPropagation()}
                    >
                      {job.txHash.slice(0, 24)}…↗
                    </a>
                  </div>
                  <div className={styles.detailRow}>
                    <span className={styles.dk}>Attestation</span>
                    <span className={styles.dv}>{job.attestationHash.slice(0, 24)}…</span>
                  </div>
                  <div className={styles.detailRow}>
                    <span className={styles.dk}>Input</span>
                    <span className={styles.dv}>{JSON.stringify(job.input)}</span>
                  </div>
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}