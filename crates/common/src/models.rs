//! Diesel model structs — one Queryable + one Insertable per table.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::{batches, jobs, models, vault_events};

// ── Model ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Queryable, Selectable, Identifiable)]
#[diesel(table_name = models)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Model {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub ipfs_cid: String,
    pub input_shape: serde_json::Value,
    pub on_chain_hash: String,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Insertable, Deserialize)]
#[diesel(table_name = models)]
pub struct NewModel {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub ipfs_cid: String,
    pub input_shape: serde_json::Value,
    pub on_chain_hash: String,
}

// ── Batch ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Queryable, Selectable, Identifiable)]
#[diesel(table_name = batches)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Batch {
    pub id: Uuid,
    pub status: String,
    pub job_count: i32,
    pub aggregated_proof_path: Option<String>,
    pub tx_hash: Option<String>,
    pub gas_used: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub aggregated_at: Option<DateTime<Utc>>,
    pub settled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = batches)]
pub struct NewBatch {
    pub id: Uuid,
    pub status: String,
}

#[derive(Debug, AsChangeset)]
#[diesel(table_name = batches)]
pub struct BatchUpdate {
    pub status: String,
    pub job_count: i32,
    pub aggregated_proof_path: Option<String>,
    pub tx_hash: Option<String>,
    pub gas_used: Option<i64>,
    pub aggregated_at: Option<DateTime<Utc>>,
    pub settled_at: Option<DateTime<Utc>>,
}

// ── Job ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Queryable, Selectable, Identifiable)]
#[diesel(table_name = jobs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Job {
    pub id: Uuid,
    pub model_id: Uuid,
    pub status: String,
    pub input_hash: String,
    pub proof_path: Option<String>,
    pub error: Option<String>,
    pub submitted_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub settled_at: Option<DateTime<Utc>>,
    pub tx_hash: Option<String>,
    pub batch_id: Option<Uuid>,
    pub attestation_hash: Option<String>,
    pub proof_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = jobs)]
pub struct NewJob {
    pub id: Uuid,
    pub model_id: Uuid,
    pub status: String,
    pub input_hash: String,
}

#[derive(Debug, AsChangeset)]
#[diesel(table_name = jobs)]
pub struct JobUpdate {
    pub status: String,
    pub proof_path: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub settled_at: Option<DateTime<Utc>>,
    pub tx_hash: Option<String>,
    pub batch_id: Option<Uuid>,
    pub attestation_hash: Option<String>,
}


pub struct AccountStats {
    pub proof_count: i64,
}
#[derive(Debug, Clone, Serialize, Queryable, Selectable)]
#[diesel(table_name = vault_events)]
pub struct VaultEvent {
    pub tx_hash:    String,
    pub operation:  String,
    pub amount_wei: String,
    pub created_at: chrono::DateTime<Utc>,
    pub job_id:     Option<Uuid>,
}