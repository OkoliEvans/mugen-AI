-- migrations/2026-04-03-000002_sp1_columns/up.sql
-- Run: diesel migration run

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- ── models ────────────────────────────────────────────────────────────────────
CREATE TABLE models (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT        NOT NULL,
    version         TEXT        NOT NULL,
    ipfs_cid        TEXT        NOT NULL,
    input_shape     JSONB       NOT NULL,
    on_chain_hash   TEXT        NOT NULL,
    registered_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(name, version)
);

-- ── batches ───────────────────────────────────────────────────────────────────
CREATE TABLE batches (
    id                    UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    status                TEXT    NOT NULL DEFAULT 'pending'
                                  CHECK (status IN ('pending','aggregating','done','failed')),
    job_count             INTEGER NOT NULL DEFAULT 0,
    aggregated_proof_path TEXT,
    tx_hash               TEXT,
    gas_used              BIGINT,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    aggregated_at         TIMESTAMPTZ,
    settled_at            TIMESTAMPTZ
);

CREATE INDEX batches_status_idx  ON batches(status);
CREATE INDEX batches_created_idx ON batches(created_at DESC);

-- ── jobs ──────────────────────────────────────────────────────────────────────
CREATE TABLE jobs (
    id               UUID    PRIMARY KEY,
    model_id         UUID    NOT NULL REFERENCES models(id),
    status           TEXT    NOT NULL DEFAULT 'queued'
                             CHECK (status IN ('queued','running','proving','done','failed','settled')),
    input_hash       TEXT    NOT NULL,
    proof_path       TEXT,
    error            TEXT,
    submitted_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at       TIMESTAMPTZ,
    completed_at     TIMESTAMPTZ,
    settled_at       TIMESTAMPTZ,
    tx_hash          TEXT,
    batch_id         UUID    REFERENCES batches(id),
    attestation_hash TEXT,
    proof_bytes      BYTEA
);

CREATE INDEX jobs_status_idx           ON jobs(status);
CREATE INDEX jobs_batch_id_idx         ON jobs(batch_id);
CREATE INDEX jobs_submitted_idx        ON jobs(submitted_at DESC);
CREATE INDEX jobs_attestation_hash_idx ON jobs(attestation_hash);