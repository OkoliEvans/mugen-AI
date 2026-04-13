-- Your SQL goes here
CREATE TABLE vault_events (
   id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
   wallet      TEXT NOT NULL,
   tx_hash     TEXT NOT NULL,
   operation   TEXT NOT NULL,  -- 'deposit' | 'deduct'
   amount_wei  TEXT NOT NULL,
   job_id      UUID REFERENCES jobs(id),
   created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
 );
 CREATE INDEX vault_events_wallet_idx ON vault_events(wallet);
 CREATE INDEX vault_events_created_at_idx ON vault_events(created_at DESC);