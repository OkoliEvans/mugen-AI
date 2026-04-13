// mugen-ui/src/pages/ProofExplorer.tsx
// — new file. Create the pages/ folder if it doesn't exist.

import { useState, useEffect, useCallback } from 'react'
import { useParams, useNavigate, Link } from 'react-router-dom'
import styles from './ProofExplorer.module.css'

const GATEWAY        = import.meta.env.VITE_GATEWAY_URL ?? '/api'
const HASHKEY_EXPLORER = 'https://testnet-explorer.hsk.xyz/tx'

// ── Types ─────────────────────────────────────────────────────────────────────

interface ProofRecord {
  job_id:           string
  status:           string
  model_id:         string
  attestation_hash: string | null
  tx_hash:          string | null
  input_hash:       string
  completed_at:     string | null
  settled_at:       string | null
}

interface ProofListResponse {
  page:   number
  limit:  number
  proofs: ProofRecord[]
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function short(s: string, n = 10) {
  if (!s) return '—'
  return `${s.slice(0, n)}…`
}

function timeAgo(iso: string | null) {
  if (!iso) return '—'
  const diff = Date.now() - new Date(iso).getTime()
  if (diff < 60_000)     return 'just now'
  if (diff < 3_600_000)  return `${Math.floor(diff / 60_000)}m ago`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`
  return `${Math.floor(diff / 86_400_000)}d ago`
}

function StatusBadge({ status }: { status: string }) {
  const cls = {
    settled:  styles.badgeSettled,
    done:     styles.badgeDone,
    proving:  styles.badgeProving,
    running:  styles.badgeRunning,
    queued:   styles.badgeQueued,
    failed:   styles.badgeFailed,
  }[status] ?? styles.badgeQueued

  return <span className={`${styles.badge} ${cls}`}>{status}</span>
}

// ── Main component ────────────────────────────────────────────────────────────

export default function ProofExplorer() {
  const { hashOrId }  = useParams<{ hashOrId?: string }>()
  const navigate      = useNavigate()

  const [query,       setQuery]       = useState(hashOrId ?? '')
  const [list,        setList]        = useState<ProofRecord[]>([])
  const [detail,      setDetail]      = useState<ProofRecord | null>(null)
  const [page,        setPage]        = useState(1)
  const [loading,     setLoading]     = useState(false)
  const [error,       setError]       = useState<string | null>(null)
  const [hasMore,     setHasMore]     = useState(true)

  const LIMIT = 20

  // ── Fetch list ─────────────────────────────────────────────────────────────

  const fetchList = useCallback(async (p: number) => {
    setLoading(true)
    setError(null)
    try {
      const res  = await fetch(`${GATEWAY}/v1/proofs?page=${p}&limit=${LIMIT}`)
      if (!res.ok) throw new Error(`Gateway error: ${res.status}`)
      const data: ProofListResponse = await res.json()
      setList(prev => p === 1 ? data.proofs : [...prev, ...data.proofs])
      setHasMore(data.proofs.length === LIMIT)
      setPage(p)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to fetch proofs')
    } finally {
      setLoading(false)
    }
  }, [])

  // ── Fetch single proof ─────────────────────────────────────────────────────

  const fetchDetail = useCallback(async (id: string) => {
    setLoading(true)
    setError(null)
    setDetail(null)
    try {
      const res = await fetch(`${GATEWAY}/v1/proofs/${encodeURIComponent(id)}`)
      if (res.status === 404) throw new Error(`No proof found for: ${id}`)
      if (!res.ok) throw new Error(`Gateway error: ${res.status}`)
      const data: ProofRecord = await res.json()
      setDetail(data)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to fetch proof')
    } finally {
      setLoading(false)
    }
  }, [])

  // ── Init ───────────────────────────────────────────────────────────────────

  useEffect(() => {
    if (hashOrId) {
      setQuery(hashOrId)
      fetchDetail(hashOrId)
    } else {
      fetchList(1)
    }
  }, [hashOrId, fetchDetail, fetchList])

  // ── Search ─────────────────────────────────────────────────────────────────

  function handleSearch(e: React.FormEvent) {
    e.preventDefault()
    const q = query.trim()
    if (!q) {
      navigate('/explorer')
      setDetail(null)
      fetchList(1)
      return
    }
    navigate(`/explorer/${q}`)
  }

  function handleClear() {
    setQuery('')
    setDetail(null)
    setError(null)
    navigate('/explorer')
    fetchList(1)
  }

  // ── Render detail ──────────────────────────────────────────────────────────

  function renderDetail(p: ProofRecord) {
    return (
      <div className={styles.detail}>
        <div className={styles.detailHeader}>
          <div className={styles.detailTitle}>Proof detail</div>
          <StatusBadge status={p.status} />
        </div>

        <div className={styles.detailGrid}>
          <Row label="Job ID"           value={p.job_id} mono copyable />
          <Row label="Status"           value={p.status} />
          <Row label="Model ID"         value={p.model_id} mono />
          <Row label="Input hash"       value={p.input_hash} mono copyable />
          <Row
            label="Attestation hash"
            value={p.attestation_hash ?? 'pending'}
            mono
            copyable={!!p.attestation_hash}
          />
          <Row
            label="Tx hash"
            value={p.tx_hash ?? 'pending batch settlement'}
            mono
            link={p.tx_hash ? `${HASHKEY_EXPLORER}/${p.tx_hash}` : undefined}
          />
          <Row label="Completed"  value={p.completed_at ? timeAgo(p.completed_at) : '—'} />
          <Row label="Settled"    value={p.settled_at   ? timeAgo(p.settled_at)   : '—'} />
        </div>

        {p.tx_hash && (
          <a
            className={styles.explorerBtn}
            href={`${HASHKEY_EXPLORER}/${p.tx_hash}`}
            target="_blank"
            rel="noreferrer"
          >
            View on HashKey explorer ↗
          </a>
        )}
      </div>
    )
  }

  // ── Render ─────────────────────────────────────────────────────────────────

  return (
    <div className={styles.root}>

      {/* ── Nav ── */}
      <nav className={styles.nav}>
        <Link to="/" className={styles.wordmark}>mugen</Link>
        <div className={styles.navLinks}>
          <Link to="/"         className={styles.navLink}>SDK</Link>
          <Link to="/explorer" className={`${styles.navLink} ${styles.navLinkActive}`}>Explorer</Link>
          <a
            className={styles.navLink}
            href="https://github.com/OkoliEvans/mugen-AI"
            target="_blank"
            rel="noreferrer"
          >
            github ↗
          </a>
        </div>
      </nav>

      {/* ── Header ── */}
      <header className={styles.header}>
        <div className={styles.headerTag}>proof explorer</div>
        <h1 className={styles.headerTitle}>
          Verify any inference.<br />
          <span className={styles.headerAccent}>On-chain.</span>
        </h1>
        <p className={styles.headerSub}>
          Search by attestation hash or job ID to verify a proof generated
          by the Mugen verifiable inference network.
        </p>
      </header>

      {/* ── Search ── */}
      <form className={styles.searchForm} onSubmit={handleSearch}>
        <input
          className={styles.searchInput}
          value={query}
          onChange={e => setQuery(e.target.value)}
          placeholder="Attestation hash (0x…) or job UUID"
          spellCheck={false}
          autoComplete="off"
        />
        <button className={styles.searchBtn} type="submit">
          Search
        </button>
        {query && (
          <button className={styles.clearBtn} type="button" onClick={handleClear}>
            ✕
          </button>
        )}
      </form>

      {/* ── Error ── */}
      {error && (
        <div className={styles.error}>
          {error}
        </div>
      )}

      {/* ── Loading ── */}
      {loading && (
        <div className={styles.loading}>
          <span className={styles.spinner} /> Fetching…
        </div>
      )}

      {/* ── Detail view ── */}
      {!loading && detail && renderDetail(detail)}

      {/* ── List view ── */}
      {!loading && !detail && !error && list.length > 0 && (
        <>
          <div className={styles.listHeader}>
            <span className={styles.listTitle}>Recent proofs</span>
            <span className={styles.listMeta}>{list.length} loaded</span>
          </div>

          <div className={styles.table}>
            <div className={styles.tableHead}>
              <span>Job ID</span>
              <span>Status</span>
              <span>Model</span>
              <span>Attestation</span>
              <span>Tx hash</span>
              <span>Settled</span>
            </div>

            {list.map(p => (
              <div
                key={p.job_id}
                className={styles.tableRow}
                onClick={() => navigate(`/explorer/${p.job_id}`)}
              >
                <span className={styles.mono}>{short(p.job_id, 8)}</span>
                <StatusBadge status={p.status} />
                <span className={styles.mono}>{short(p.model_id, 12)}</span>
                <span className={styles.mono}>
                  {p.attestation_hash ? short(p.attestation_hash, 10) : '—'}
                </span>
                <span className={styles.mono}>
                  {p.tx_hash ? short(p.tx_hash, 10) : '—'}
                </span>
                <span className={styles.muted}>{timeAgo(p.settled_at)}</span>
              </div>
            ))}
          </div>

          {hasMore && (
            <button
              className={styles.loadMore}
              onClick={() => fetchList(page + 1)}
              disabled={loading}
            >
              Load more
            </button>
          )}
        </>
      )}

      {/* ── Empty ── */}
      {!loading && !error && !detail && list.length === 0 && (
        <div className={styles.empty}>
          No proofs yet — run an inference to generate the first one.
        </div>
      )}

      <footer className={styles.footer}>
        <span>© 2026 Mist Labs</span>
        <span>·</span>
        <a href="https://testnet-explorer.hsk.xyz" target="_blank" rel="noreferrer">
          HashKey explorer ↗
        </a>
      </footer>
    </div>
  )
}

// ── Row sub-component ─────────────────────────────────────────────────────────

function Row({
  label, value, mono, copyable, link
}: {
  label:     string
  value:     string
  mono?:     boolean
  copyable?: boolean
  link?:     string
}) {
  const [copied, setCopied] = useState(false)

  function copy() {
    navigator.clipboard.writeText(value)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  return (
    <div className={styles.row}>
      <span className={styles.rowLabel}>{label}</span>
      <span className={`${styles.rowValue} ${mono ? styles.mono : ''}`}>
        {link ? (
          <a href={link} target="_blank" rel="noreferrer" className={styles.rowLink}>
            {value} ↗
          </a>
        ) : value}
        {copyable && (
          <button className={styles.copyBtn} onClick={copy}>
            {copied ? '✓' : 'copy'}
          </button>
        )}
      </span>
    </div>
  )
}