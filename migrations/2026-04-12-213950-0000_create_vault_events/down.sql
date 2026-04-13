-- This file should undo anything in `up.sql`
-- Drop indexes first (important for clean rollback)
DROP INDEX IF EXISTS vault_events_created_at_idx;
DROP INDEX IF EXISTS vault_events_wallet_idx;

-- Drop table
DROP TABLE IF EXISTS vault_events;