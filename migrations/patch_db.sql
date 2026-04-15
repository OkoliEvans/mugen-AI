-- patch_db.sql
-- Run against your existing database to apply the SP1 + analytics schema migration.
-- Usage: psql $DATABASE_URL -f patch_db.sql
--
-- Safe to run multiple times — all operations are idempotent.

BEGIN;

-- ─────────────────────────────────────────────
-- 1. Extend jobs table (SP1 additions)
-- ─────────────────────────────────────────────
ALTER TABLE jobs
    ADD COLUMN IF NOT EXISTS attestation_hash TEXT,
    ADD COLUMN IF NOT EXISTS proof_bytes       BYTEA;

CREATE INDEX IF NOT EXISTS jobs_attestation_hash_idx ON jobs(attestation_hash);

-- widen status constraint
ALTER TABLE jobs DROP CONSTRAINT IF EXISTS jobs_status_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_status_check
    CHECK (status IN ('queued', 'running', 'proving', 'done', 'failed', 'settled'));


-- ─────────────────────────────────────────────
-- 2. Create bets table
-- ─────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS bets (
    id UUID PRIMARY KEY,
    job_id UUID REFERENCES jobs(id) ON DELETE SET NULL,
    market_id TEXT NOT NULL,
    question TEXT NOT NULL,
    side TEXT NOT NULL,
    size_usdc DOUBLE PRECISION NOT NULL,
    price DOUBLE PRECISION NOT NULL,
    paper BOOLEAN NOT NULL,
    confidence DOUBLE PRECISION NOT NULL,
    yes_price DOUBLE PRECISION NOT NULL,
    no_price DOUBLE PRECISION NOT NULL,
    volume_24h DOUBLE PRECISION NOT NULL,
    attestation_hash TEXT,
    tx_hash TEXT,
    outcome BOOLEAN,
    pnl_usdc DOUBLE PRECISION,
    placed_at TIMESTAMPTZ NOT NULL,
    resolved_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS bets_job_id_idx ON bets(job_id);


-- ─────────────────────────────────────────────
-- 3. Create market_snapshots
-- ─────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS market_snapshots (
    id UUID PRIMARY KEY,
    market_id TEXT NOT NULL,
    question TEXT NOT NULL,
    yes_price DOUBLE PRECISION NOT NULL,
    no_price DOUBLE PRECISION NOT NULL,
    volume_24h DOUBLE PRECISION NOT NULL,
    end_date TIMESTAMPTZ NOT NULL,
    captured_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS market_snapshots_market_id_idx ON market_snapshots(market_id);


-- ─────────────────────────────────────────────
-- 4. Create outcomes
-- ─────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS outcomes (
    id UUID PRIMARY KEY,
    market_id TEXT NOT NULL,
    question TEXT NOT NULL,
    resolved_at TIMESTAMPTZ NOT NULL,
    outcome BOOLEAN NOT NULL
);


-- ─────────────────────────────────────────────
-- 5. Create training_samples
-- ─────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS training_samples (
    id UUID PRIMARY KEY,
    snapshot_id UUID NOT NULL REFERENCES market_snapshots(id) ON DELETE CASCADE,
    outcome_id UUID NOT NULL REFERENCES outcomes(id) ON DELETE CASCADE,
    market_id TEXT NOT NULL,
    yes_price DOUBLE PRECISION NOT NULL,
    no_price DOUBLE PRECISION NOT NULL,
    volume_24h DOUBLE PRECISION NOT NULL,
    time_to_expiry DOUBLE PRECISION NOT NULL,
    outcome BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS training_samples_snapshot_idx ON training_samples(snapshot_id);
CREATE INDEX IF NOT EXISTS training_samples_outcome_idx ON training_samples(outcome_id);


-- ─────────────────────────────────────────────
-- 6. Verify critical schema
-- ─────────────────────────────────────────────
DO $$
DECLARE
    has_attestation_hash BOOLEAN;
    has_proof_bytes      BOOLEAN;
    has_bets             BOOLEAN;
    has_snapshots        BOOLEAN;
    has_outcomes         BOOLEAN;
    has_training         BOOLEAN;
BEGIN
    SELECT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'jobs' AND column_name = 'attestation_hash'
    ) INTO has_attestation_hash;

    SELECT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'jobs' AND column_name = 'proof_bytes'
    ) INTO has_proof_bytes;

    SELECT EXISTS (
        SELECT 1 FROM information_schema.tables
        WHERE table_name = 'bets'
    ) INTO has_bets;

    SELECT EXISTS (
        SELECT 1 FROM information_schema.tables
        WHERE table_name = 'market_snapshots'
    ) INTO has_snapshots;

    SELECT EXISTS (
        SELECT 1 FROM information_schema.tables
        WHERE table_name = 'outcomes'
    ) INTO has_outcomes;

    SELECT EXISTS (
        SELECT 1 FROM information_schema.tables
        WHERE table_name = 'training_samples'
    ) INTO has_training;

    IF has_attestation_hash 
       AND has_proof_bytes 
       AND has_bets 
       AND has_snapshots 
       AND has_outcomes 
       AND has_training THEN
        RAISE NOTICE 'Migration OK — full schema aligned with code.';
    ELSE
        RAISE EXCEPTION 'Migration check failed — schema mismatch detected.';
    END IF;
END $$;

COMMIT;