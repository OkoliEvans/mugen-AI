import { useState, useEffect, useCallback } from 'react'

export interface HealthState {
  status: 'checking' | 'healthy' | 'unreachable'
  version?: string
  settleEnabled?: boolean
}

export interface JobRecord {
  jobId:           string
  txHash:          string
  attestationHash: string
  elapsedMs:       number
  modelId:         string
  input:           number[][]
  settledAt:       string
}

const GATEWAY = import.meta.env.VITE_GATEWAY_URL ?? '/api'
const HISTORY_KEY = 'mugen_job_history'

function loadHistory(): JobRecord[] {
  try {
    return JSON.parse(localStorage.getItem(HISTORY_KEY) ?? '[]')
  } catch { return [] }
}

function saveHistory(jobs: JobRecord[]) {
  localStorage.setItem(HISTORY_KEY, JSON.stringify(jobs.slice(0, 50)))
}

export function useGateway() {
  const [health, setHealth] = useState<HealthState>({ status: 'checking' })
  const [history, setHistory] = useState<JobRecord[]>(loadHistory)

  const checkHealth = useCallback(async () => {
    setHealth({ status: 'checking' })
    try {
      const res = await fetch(`${GATEWAY}/healthz`)
      if (!res.ok) throw new Error()
      const data = await res.json()
      setHealth({
        status:        'healthy',
        version:       data.version,
        settleEnabled: data.settle_enabled,
      })
    } catch {
      setHealth({ status: 'unreachable' })
    }
  }, [])

  useEffect(() => { checkHealth() }, [checkHealth])

  const addJob = useCallback((job: JobRecord) => {
    setHistory(prev => {
      const next = [job, ...prev].slice(0, 50)
      saveHistory(next)
      return next
    })
  }, [])

  const clearHistory = useCallback(() => {
    setHistory([])
    localStorage.removeItem(HISTORY_KEY)
  }, [])

  return { health, checkHealth, history, addJob, clearHistory }
}