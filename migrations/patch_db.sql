-- patch_db.sql
-- Run against your existing database to apply the SP1 migration manually.
-- Usage: psql $DATABASE_URL -f patch_db.sql
--
-- Safe to run multiple times — all operations are idempotent.

BEGIN;

-- 1. New columns
ALTER TABLE jobs
    ADD COLUMN IF NOT EXISTS attestation_hash TEXT,
    ADD COLUMN IF NOT EXISTS proof_bytes       BYTEA;

-- 2. Index for attestation_hash lookups
CREATE INDEX IF NOT EXISTS jobs_attestation_hash_idx ON jobs(attestation_hash);

-- 3. Widen status CHECK to include 'proving'
ALTER TABLE jobs DROP CONSTRAINT IF EXISTS jobs_status_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_status_check
    CHECK (status IN ('queued', 'running', 'proving', 'done', 'failed', 'settled'));

-- 4. Verify
DO $$
DECLARE
    has_attestation_hash BOOLEAN;
    has_proof_bytes      BOOLEAN;
BEGIN
    SELECT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'jobs' AND column_name = 'attestation_hash'
    ) INTO has_attestation_hash;

    SELECT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'jobs' AND column_name = 'proof_bytes'
    ) INTO has_proof_bytes;

    IF has_attestation_hash AND has_proof_bytes THEN
        RAISE NOTICE 'Migration OK — attestation_hash and proof_bytes present.';
    ELSE
        RAISE EXCEPTION 'Migration check failed — columns missing.';
    END IF;
END $$;

COMMIT;