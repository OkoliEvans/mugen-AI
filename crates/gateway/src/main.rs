//! Veil Gateway — Actix-Web HTTP API

use std::sync::Arc;

use actix_cors::Cors;
use actix_web::{
    App, HttpResponse, HttpServer, Responder, get, http::header, post, web::{Data, Json, Path, Query}
};
use chrono::Utc;
use common::{
    models::{JobUpdate, NewJob, NewModel},
    repo, DbPool,
};
use ipfs::{PinMeta, PinataClient};
use prover_manager::{
    job::JobState,
    manager::{ProverConfig, ProverManager},
};
use serde::{Deserialize, Serialize};
use settler::{config::SettlerConfig, settler::Settler};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct AccountResponse {
    wallet: String,
    vault_balance_wei: String,
    vault_balance_hsk: String,
    proof_count: i64,
    hsk_spent: String,
    proofs_remaining: i64,
}

#[derive(Debug, Serialize)]
struct HistoryEntryResponse {
    tx_hash: String,
    operation: String,
    amount: String,
    timestamp: String,
    job_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    #[serde(default = "default_page")]
    page: u64,
    #[serde(default = "default_limit")]
    limit: u64,
    operation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RecordVaultEventRequest {
    tx_hash: String,
    amount_wei: String,
    operation: String,
}

#[derive(Debug, Serialize)]
struct ProofListItem {
    job_id: String,
    status: String,
    model_id: String,
    attestation_hash: Option<String>,
    tx_hash: Option<String>,
    input_hash: String,
    completed_at: Option<String>,
    settled_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProofListQuery {
    #[serde(default = "default_page")]
    page: u64,
    #[serde(default = "default_limit")]
    limit: u64,
}

fn default_page() -> u64 {
    1
}
fn default_limit() -> u64 {
    20
}

// ── Config ────────────────────────────────────────────────────────────────────

fn prover_config() -> ProverConfig {
    ProverConfig {
        proofs_dir: std::env::var("PROOFS_DIR").unwrap_or_else(|_| "/tmp/mugen-proofs".into()),
        max_concurrent: std::env::var("MAX_CONCURRENT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1),
        timeout_secs: std::env::var("TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300),
        guest_elf_path: std::env::var("GUEST_ELF_PATH")
            .unwrap_or_else(|_| "crates/guest/elf/inference-guest".into()),
        weights_path: std::env::var("MODEL_WEIGHTS_PATH")
            .unwrap_or_else(|_| "weights/tiny_mlp.bin".into()),
        job_ttl_secs: std::env::var("JOB_TTL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600),
    }
}

struct BatchConfig {
    batch_size: usize,
    flush_secs: u64,
    agg_elf_path: String,
}

impl BatchConfig {
    fn from_env() -> Self {
        Self {
            batch_size: std::env::var("BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            flush_secs: std::env::var("BATCH_FLUSH_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            agg_elf_path: std::env::var("AGG_ELF_PATH")
                .unwrap_or_else(|_| "crates/aggregator-guest/elf/aggregator-guest".into()),
        }
    }
}

// ── Batch collector ───────────────────────────────────────────────────────────

struct BatchCollector {
    /// (job_id, AggregationInput) so we can update DB rows after settlement.
    pending: Mutex<Vec<(Uuid, aggregator::AggregationInput)>>,
    batch_size: usize,
    flush_secs: u64,
    agg_elf: Vec<u8>,
}

impl BatchCollector {
    async fn new(cfg: &BatchConfig) -> anyhow::Result<Arc<Self>> {
        let agg_elf = tokio::fs::read(&cfg.agg_elf_path).await.map_err(|e| {
            anyhow::anyhow!(
                "failed to read aggregator ELF at '{}': {e}",
                cfg.agg_elf_path
            )
        })?;

        info!(
            batch_size   = cfg.batch_size,
            flush_secs   = cfg.flush_secs,
            agg_elf_path = %cfg.agg_elf_path,
            "batch collector initialised"
        );

        Ok(Arc::new(Self {
            pending: Mutex::new(Vec::new()),
            batch_size: cfg.batch_size,
            flush_secs: cfg.flush_secs,
            agg_elf,
        }))
    }

    /// Push one proof. Returns `Some(batch)` when threshold is reached.
    async fn push(
        &self,
        db_job_id: Uuid,
        input: aggregator::AggregationInput,
    ) -> Option<Vec<(Uuid, aggregator::AggregationInput)>> {
        let mut pending = self.pending.lock().await;
        pending.push((db_job_id, input));
        if pending.len() >= self.batch_size {
            Some(std::mem::take(&mut *pending))
        } else {
            None
        }
    }

    async fn flush(&self) -> Vec<(Uuid, aggregator::AggregationInput)> {
        std::mem::take(&mut *self.pending.lock().await)
    }

    async fn pending_count(&self) -> usize {
        self.pending.lock().await.len()
    }
}

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    manager: Arc<ProverManager>,
    settler: Arc<Settler>,
    pool: Arc<DbPool>,
    settle_enabled: bool,
    ipfs_client: Option<Arc<PinataClient>>,
    batch_collector: Arc<BatchCollector>,
    vault: Option<Arc<settler::vault::VaultClient>>,
}

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SubmitRequest {
    input_data: Vec<Vec<f64>>,
    #[serde(default = "default_model_id")]
    model_id: String,
    wallet_address: Option<String>,
}

fn default_model_id() -> String {
    "tiny_mlp_v1".to_string()
}

#[derive(Debug, Serialize)]
struct SubmitResponse {
    job_id: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct JobStatusResponse {
    job_id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    proof_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attestation_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProofResponse {
    job_id: String,
    proof_hex: String,
    size_bytes: usize,
}

#[derive(Debug, Deserialize)]
struct RegisterModelRequest {
    name: String,
    version: String,
    artifact_b64: String,
    input_shape: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct RegisterModelResponse {
    model_id: String,
    on_chain_model_id: String,
    ipfs_cid: String,
    gateway_url: String,
    on_chain_hash: String,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    settle_enabled: bool,
    db: &'static str,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

fn err(msg: impl Into<String>) -> Json<ErrorResponse> {
    Json(ErrorResponse { error: msg.into() })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn upsert_model(
    pool: &DbPool,
    name: &str,
    input_data: &[Vec<f64>],
) -> Result<(Uuid, String), String> {
    let shape = serde_json::json!([
        input_data.len(),
        input_data.first().map(|r| r.len()).unwrap_or(0)
    ]);

    match repo::find_model_by_name(pool, name).await {
        Ok(m) => {
            if m.input_shape == serde_json::json!([]) {
                if let Err(e) = repo::update_model_shape(pool, m.id, shape).await {
                    warn!("failed to patch input_shape for model {}: {e}", m.id);
                }
            }
            return Ok((m.id, m.version));
        }
        Err(common::error::CommonError::NotFound(_)) => {}
        Err(e) => return Err(e.to_string()),
    }

    let version = "0.1.0".to_string();
    let new = NewModel {
        id: Uuid::new_v4(),
        name: name.to_string(),
        version: version.clone(),
        ipfs_cid: "pending".to_string(),
        input_shape: shape,
        on_chain_hash: "pending".to_string(),
    };

    repo::insert_model(pool, new)
        .await
        .map(|m| (m.id, version))
        .map_err(|e| e.to_string())
}

fn hash_input(input_data: &[Vec<f64>]) -> String {
    let bytes = serde_json::to_vec(input_data).unwrap_or_default();
    hex::encode(Sha256::digest(&bytes))
}

// ── Aggregation + settlement ──────────────────────────────────────────────────

/// Aggregate a batch and settle on-chain.
/// After settlement, updates every job row in the batch to status="settled".
async fn settle_batch(
    batch: Vec<(Uuid, aggregator::AggregationInput)>,
    agg_elf: &[u8],
    settler: &Arc<Settler>,
    pool: &Arc<DbPool>,
) {
    let count = batch.len();
    info!(count, "aggregating proof batch");

    // Split db_job_ids from AggregationInputs
    let (db_job_ids, inputs): (Vec<Uuid>, Vec<aggregator::AggregationInput>) =
        batch.into_iter().unzip();

    let agg_data = match aggregator::aggregate_proofs(agg_elf, inputs).await {
        Ok(d) => d,
        Err(e) => {
            error!(count, "aggregation failed: {e}");
            return;
        }
    };

    let merkle_root = hex::encode(aggregator::merkle_root(&agg_data.output_hashes));
    info!(
        count,
        batch_size  = agg_data.batch_size,
        merkle_root = %merkle_root,
        "aggregation complete — settling on-chain"
    );

    let proof_path = format!("/tmp/mugen-proofs/agg_{}.bin", Uuid::new_v4());
    if let Err(e) = tokio::fs::create_dir_all("/tmp/mugen-proofs").await {
        error!("failed to create proofs dir: {e}");
        return;
    }
    if let Err(e) = tokio::fs::write(&proof_path, &agg_data.proof_bytes).await {
        error!("failed to write aggregated proof to {proof_path}: {e}");
        return;
    }

    let tx_hash = match settler
        .submit_aggregated(&proof_path, &agg_data.output_hashes)
        .await
    {
        Ok(h) if h == "already-verified" => {
            warn!(count, "aggregated batch already settled — skipping");
            let _ = tokio::fs::remove_file(&proof_path).await;
            return;
        }
        Ok(h) => {
            info!(count, tx_hash = %h, merkle_root = %merkle_root, "batch settled on-chain ✓");
            h
        }
        Err(e) => {
            error!(count, "batch settlement failed: {e}");
            let _ = tokio::fs::remove_file(&proof_path).await;
            return;
        }
    };

    let _ = tokio::fs::remove_file(&proof_path).await;

    // ── Update every job in this batch to settled ─────────────────────────────
    for db_job_id in &db_job_ids {
        if let Err(e) = repo::update_job(
            pool,
            *db_job_id,
            JobUpdate {
                status: "settled".into(),
                proof_path: None,
                error: None,
                started_at: None,
                completed_at: None,
                settled_at: Some(Utc::now()),
                tx_hash: Some(tx_hash.clone()),
                batch_id: None,
                attestation_hash: None,
            },
        )
        .await
        {
            error!(job_id = %db_job_id, "DB update to settled failed: {e}");
        } else {
            info!(job_id = %db_job_id, %tx_hash, "job marked settled in DB");
        }
    }
}

// ── Settlement background task ────────────────────────────────────────────────
fn spawn_settler(
    state: AppState,
    job_id: String,
    db_job_id: Uuid,
    model_name: String,
    model_version: String,
    input_data: Vec<Vec<f64>>,
    wallet_address: Option<String>,
) {
    tokio::spawn(async move {
        if let Err(e) = repo::update_job(
            &state.pool,
            db_job_id,
            JobUpdate {
                status: "running".into(),
                proof_path: None,
                error: None,
                started_at: Some(Utc::now()),
                completed_at: None,
                settled_at: None,
                tx_hash: None,
                batch_id: None,
                attestation_hash: None,
            },
        )
        .await
        {
            error!(%job_id, %db_job_id, "DB update to running failed: {e}");
        }

        let mut attempts = 0u32;
        let max_attempts = 480u32;

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            attempts += 1;

            match state.manager.status(&job_id).await {
                Ok(JobState::Compressed { attestation_hash }) => {
                    let hash_hex = format!("0x{}", hex::encode(attestation_hash));
                    info!(
                        %job_id,
                        attestation_hash = %hash_hex,
                        "compressed proof ready — persisting attestation_hash"
                    );

                    // Persist proving status + attestation_hash
                    if let Err(e) = repo::update_job(
                        &state.pool,
                        db_job_id,
                        JobUpdate {
                            status: "proving".into(),
                            proof_path: None,
                            error: None,
                            started_at: None,
                            completed_at: None,
                            settled_at: None,
                            tx_hash: None,
                            batch_id: None,
                            attestation_hash: Some(hash_hex.clone()),
                        },
                    )
                    .await
                    {
                        error!(%job_id, %db_job_id, "DB update to proving failed: {e}");
                    }

                    // ── Fee deduction ─────────────────────────────────────────────────
                    // Deduct AFTER proof is submitted to Succinct — not before.
                    // Failed proofs never reach Compressed state so users are never
                    // charged for proofs that didn't succeed.
                    if let (Some(vault), Some(wallet)) = (&state.vault, &wallet_address) {
                        match vault
                            .deduct_fee(wallet, settler::vault::ProofTier::Standard, &job_id)
                            .await
                        {
                            Ok(fee_tx) => {
                                info!(%job_id, %fee_tx, "vault fee deducted");
                                // Record the deduct event so vault_events is populated
                                // and GET /v1/account/:wallet/history returns real data.
                                if let Err(e) = repo::insert_vault_event(
                                    &state.pool,
                                    wallet,
                                    &fee_tx,
                                    "deduct",
                                    "2000000000000000000", // 2 HSK in wei
                                    Some(db_job_id),
                                )
                                .await
                                {
                                    warn!(%job_id, "insert_vault_event failed: {e}");
                                }
                            }
                            Err(e) => {
                                // Log but don't fail the proof — it's already proven.
                                // This should not happen if the balance check at
                                // submit_job time passed.
                                error!(%job_id, "vault fee deduction failed: {e} — proof continues");
                            }
                        }
                    }

                    // ── Push to batch collector ───────────────────────────────────────
                    if state.settle_enabled && std::env::var("SP1_PROVER").as_deref() != Ok("mock")
                    {
                        match state.manager.compressed_proof_data(&job_id).await {
                            Ok(cd) => {
                                let agg_input = aggregator::AggregationInput {
                                    proof: cd.proof,
                                    vk: cd.vk,
                                };

                                let pending = state.batch_collector.pending_count().await + 1;
                                info!(
                                    %job_id,
                                    pending,
                                    batch_size = state.batch_collector.batch_size,
                                    "pushing proof to batch collector"
                                );

                                if let Some(batch) =
                                    state.batch_collector.push(db_job_id, agg_input).await
                                {
                                    info!(
                                        count = batch.len(),
                                        "batch threshold reached — spawning aggregation"
                                    );
                                    let elf = state.batch_collector.agg_elf.clone();
                                    let settler = Arc::clone(&state.settler);
                                    let pool = Arc::clone(&state.pool);
                                    tokio::spawn(async move {
                                        settle_batch(batch, &elf, &settler, &pool).await;
                                    });
                                }
                            }
                            Err(e) => {
                                error!(
                                    %job_id,
                                    "compressed_proof_data not available after Compressed: {e}"
                                );
                            }
                        }
                    }

                    // Mark done — settle_batch will update to "settled" after batch lands
                    if let Err(e) = repo::update_job(
                        &state.pool,
                        db_job_id,
                        JobUpdate {
                            status: "done".into(),
                            proof_path: None,
                            error: None,
                            started_at: None,
                            completed_at: Some(Utc::now()),
                            settled_at: None,
                            tx_hash: None,
                            batch_id: None,
                            attestation_hash: Some(hash_hex),
                        },
                    )
                    .await
                    {
                        error!(%job_id, %db_job_id, "DB update to done failed: {e}");
                    }

                    info!(%job_id, "job complete — queued in aggregator batch");
                    return;
                }

                Ok(JobState::Failed { reason }) => {
                    warn!(%job_id, %reason, "job failed — persisting to DB");
                    if let Err(e) = repo::update_job(
                        &state.pool,
                        db_job_id,
                        JobUpdate {
                            status: "failed".into(),
                            proof_path: None,
                            error: Some(reason),
                            started_at: None,
                            completed_at: Some(Utc::now()),
                            settled_at: None,
                            tx_hash: None,
                            batch_id: None,
                            attestation_hash: None,
                        },
                    )
                    .await
                    {
                        error!(%job_id, %db_job_id, "DB update to failed: {e}");
                    }
                    return;
                }

                Ok(_) => {
                    if attempts >= max_attempts {
                        warn!(%job_id, "settler timed out waiting for compressed proof");
                        if let Err(e) = repo::update_job(
                            &state.pool,
                            db_job_id,
                            JobUpdate {
                                status: "failed".into(),
                                proof_path: None,
                                error: Some("prover timeout — exceeded max poll attempts".into()),
                                started_at: None,
                                completed_at: Some(Utc::now()),
                                settled_at: None,
                                tx_hash: None,
                                batch_id: None,
                                attestation_hash: None,
                            },
                        )
                        .await
                        {
                            error!(%job_id, %db_job_id, "DB update to timeout-failed: {e}");
                        }
                        return;
                    }
                }

                Err(e) => {
                    error!(%job_id, "settler: status poll error: {e}");
                    return;
                }
            }
        }
    });
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /v1/models
#[post("/v1/models")]
async fn register_model(
    state: Data<AppState>,
    Json(req): Json<RegisterModelRequest>,
) -> impl Responder {
    if req.name.trim().is_empty() || req.version.trim().is_empty() {
        return HttpResponse::BadRequest().json(err("name and version must not be empty"));
    }
    if req.artifact_b64.is_empty() {
        return HttpResponse::BadRequest().json(err("artifact_b64 must not be empty"));
    }
    if req.input_shape.is_empty() {
        return HttpResponse::BadRequest().json(err("input_shape must not be empty"));
    }

    let ipfs = match state.ipfs_client.as_ref() {
        Some(c) => c,
        None => {
            return HttpResponse::ServiceUnavailable().json(err(
                "IPFS not configured — set PINATA_JWT and PINATA_GATEWAY_URL",
            ));
        }
    };

    match repo::find_model_by_name(&state.pool, &req.name).await {
        Ok(m) if m.ipfs_cid != "pending" => {
            return HttpResponse::Conflict().json(err(format!(
                "model '{}' is already registered (ipfs_cid: {})",
                req.name, m.ipfs_cid
            )));
        }
        Ok(_) | Err(common::error::CommonError::NotFound(_)) => {}
        Err(e) => {
            error!("DB lookup failed during model registration: {e}");
            return HttpResponse::InternalServerError().json(err("database error"));
        }
    }

    let artifact_bytes = match base64::decode(&req.artifact_b64) {
        Ok(b) => bytes::Bytes::from(b),
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(err(format!("invalid base64 in artifact_b64: {e}")));
        }
    };

    info!(
        model_name    = %req.name,
        model_version = %req.version,
        artifact_size = artifact_bytes.len(),
        "registering model"
    );

    let filename = format!("{}_{}.onnx", req.name, req.version.replace('.', "_"));
    let meta = PinMeta {
        name: format!("{}@{}", req.name, req.version),
        keyvalues: Some(serde_json::json!({
            "model_name":    req.name,
            "model_version": req.version,
        })),
    };

    let artifact_bytes_for_model_id = artifact_bytes.clone();
    let ipfs_cid = match ipfs.pin_bytes(artifact_bytes, &filename, Some(meta)).await {
        Ok(cid) => cid,
        Err(e) => {
            error!(model_name = %req.name, "IPFS pin failed: {e}");
            return HttpResponse::BadGateway().json(err(format!("IPFS pin failed: {e}")));
        }
    };

    let gateway_url = ipfs.gateway_url(&ipfs_cid);
    info!(model_name = %req.name, %ipfs_cid, "artifact pinned to IPFS");

    if !state.settle_enabled {
        warn!(model_name = %req.name, "settlement disabled — skipping on-chain registration");
        return match upsert_registered_model(
            &state.pool,
            &req.name,
            &req.version,
            &req.input_shape,
            &ipfs_cid,
            "settlement-disabled",
        )
        .await
        {
            Ok((model_id, on_chain_model_id)) => {
                HttpResponse::Created().json(RegisterModelResponse {
                    model_id: model_id.to_string(),
                    on_chain_model_id,
                    ipfs_cid,
                    gateway_url,
                    on_chain_hash: "settlement-disabled".into(),
                })
            }
            Err(e) => HttpResponse::InternalServerError().json(err(e)),
        };
    }

    let on_chain_hash = match state
        .settler
        .register_model(
            &req.name,
            &req.version,
            &ipfs_cid,
            &req.input_shape,
            &artifact_bytes_for_model_id,
        )
        .await
    {
        Ok(tx_hash) => tx_hash,
        Err(e) => {
            error!(model_name = %req.name, "on-chain registration failed: {e}");
            return HttpResponse::BadGateway()
                .json(err(format!("on-chain registration failed: {e}")));
        }
    };

    info!(model_name = %req.name, %on_chain_hash, "model registered on HashKey testnet");

    // FIX: both ipfs_cid and on_chain_hash are moved into the Ok arm's
    // RegisterModelResponse. Clone them so the Err arm can still use them.
    let ipfs_cid_for_err = ipfs_cid.clone();
    let on_chain_hash_for_err = on_chain_hash.clone();

    match upsert_registered_model(
        &state.pool,
        &req.name,
        &req.version,
        &req.input_shape,
        &ipfs_cid,
        &on_chain_hash,
    )
    .await
    {
        Ok((model_id, on_chain_model_id)) => {
            info!(model_name = %req.name, %model_id, "model registration complete");
            HttpResponse::Created().json(RegisterModelResponse {
                model_id: model_id.to_string(),
                on_chain_model_id,
                ipfs_cid,
                gateway_url,
                on_chain_hash,
            })
        }
        Err(e) => {
            error!(model_name = %req.name, "DB persist failed after on-chain registration: {e}");
            HttpResponse::MultiStatus().json(serde_json::json!({
                "warning":       "model registered on-chain but DB update failed",
                "ipfs_cid":      ipfs_cid_for_err,       // clone used here
                "on_chain_hash": on_chain_hash_for_err,  // clone used here
                "error":         e,
            }))
        }
    }
}

async fn upsert_registered_model(
    pool: &DbPool,
    name: &str,
    version: &str,
    input_shape: &[u64],
    ipfs_cid: &str,
    on_chain_hash: &str,
) -> Result<(Uuid, String), String> {
    let shape = serde_json::json!(input_shape);

    use alloy::primitives::keccak256;
    let mut model_id_input = Vec::with_capacity(name.len() + version.len());
    model_id_input.extend_from_slice(name.as_bytes());
    model_id_input.extend_from_slice(version.as_bytes());
    let on_chain_model_id = format!("0x{}", hex::encode(keccak256(&model_id_input)));

    match repo::find_model_by_name(pool, name).await {
        Ok(m) => {
            repo::update_model_registration(
                pool,
                m.id,
                ipfs_cid.to_string(),
                on_chain_hash.to_string(),
            )
            .await
            .map_err(|e| e.to_string())?;

            if m.input_shape == serde_json::json!([]) {
                let _ = repo::update_model_shape(pool, m.id, shape).await;
            }

            Ok((m.id, on_chain_model_id))
        }
        Err(common::error::CommonError::NotFound(_)) => {
            let id = Uuid::new_v4();
            let new = NewModel {
                id,
                name: name.to_string(),
                version: version.to_string(),
                ipfs_cid: ipfs_cid.to_string(),
                input_shape: shape,
                on_chain_hash: on_chain_hash.to_string(),
            };
            repo::insert_model(pool, new)
                .await
                .map(|m| (m.id, on_chain_model_id))
                .map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// GET /healthz
#[get("/healthz")]
async fn healthz(state: Data<AppState>) -> impl Responder {
    HttpResponse::Ok().json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        settle_enabled: state.settle_enabled,
        db: "connected",
    })
}

/// POST /v1/jobs
#[post("/v1/jobs")]
async fn submit_job(state: Data<AppState>, Json(req): Json<SubmitRequest>) -> impl Responder {
    if req.input_data.is_empty() {
        return HttpResponse::BadRequest().json(err("input_data must not be empty"));
    }

    // ── VeilVault balance check ───────────────────────────────────────────────
    if let Some(vault) = &state.vault {
        match &req.wallet_address {
            None => {
                return HttpResponse::BadRequest().json(err(
                    "wallet_address is required when VeilVault fee collection is enabled",
                ));
            }
            Some(wallet) => {
                match vault
                    .check_balance(wallet, settler::vault::ProofTier::Standard)
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        let balance = vault
                            .balance_of(wallet)
                            .await
                            .map(|b| b.to_string())
                            .unwrap_or_else(|_| "0".into());
                        return HttpResponse::PaymentRequired().json(serde_json::json!({
                            "error":    "insufficient VeilVault balance",
                            "required": "2000000000000000000",
                            "balance":  balance,
                            "hint":     "deposit HSK via VeilVault.deposit{value: N ether}()"
                        }));
                    }
                    Err(e) => {
                        warn!("vault balance check failed for {wallet}: {e} — allowing job");
                    }
                }
            }
        }
    }

    let model_name = req.model_id.clone();
    let input_data = req.input_data.clone();
    let wallet_address = req.wallet_address.clone();

    let (model_uuid, model_version) =
        match upsert_model(&state.pool, &model_name, &input_data).await {
            Ok(v) => v,
            Err(e) => {
                error!("model upsert failed: {e}");
                return HttpResponse::InternalServerError()
                    .json(err(format!("model registry error: {e}")));
            }
        };

    let job_id = match state.manager.submit(input_data.clone()).await {
        Ok(id) => id,
        Err(e) => {
            error!("submit failed: {e}");
            return HttpResponse::InternalServerError().json(err(e.to_string()));
        }
    };

    let db_job_id = match Uuid::parse_str(&job_id) {
        Ok(id) => id,
        Err(e) => {
            error!("invalid job_id UUID: {e}");
            return HttpResponse::InternalServerError().json(err("internal id error"));
        }
    };

    let input_hash = hash_input(&input_data);
    let new_job = NewJob {
        id: db_job_id,
        model_id: model_uuid,
        status: "queued".into(),
        input_hash,
    };

    if let Err(e) = repo::insert_job(&state.pool, new_job).await {
        error!(%job_id, "failed to persist job to DB: {e}");
    }

    info!(%job_id, %model_name, "job submitted and persisted");

    // FIX: pass wallet_address as the 7th argument
    spawn_settler(
        state.get_ref().clone(),
        job_id.clone(),
        db_job_id,
        model_name,
        model_version,
        input_data,
        wallet_address,
    );

    HttpResponse::Accepted().json(SubmitResponse {
        job_id,
        status: "queued",
    })
}

/// GET /v1/jobs/{id}
#[get("/v1/jobs/{id}")]
async fn get_job_status(state: Data<AppState>, path: Path<String>) -> impl Responder {
    let job_id_str = path.into_inner();

    let db_id = match Uuid::parse_str(&job_id_str) {
        Ok(id) => id,
        Err(_) => {
            return HttpResponse::BadRequest().json(err("invalid job id format"));
        }
    };

    if let Ok(job) = repo::find_job(&state.pool, db_id).await {
        return HttpResponse::Ok().json(JobStatusResponse {
            job_id: job_id_str,
            status: job.status,
            proof_path: job.proof_path,
            attestation_hash: job.attestation_hash,
            tx_hash: job.tx_hash,
            reason: job.error,
        });
    }

    match state.manager.status(&job_id_str).await {
        Ok(job_state) => {
            let response = match job_state {
                JobState::Queued => JobStatusResponse {
                    job_id: job_id_str,
                    status: "queued".into(),
                    proof_path: None,
                    attestation_hash: None,
                    tx_hash: None,
                    reason: None,
                },
                JobState::Running => JobStatusResponse {
                    job_id: job_id_str,
                    status: "running".into(),
                    proof_path: None,
                    attestation_hash: None,
                    tx_hash: None,
                    reason: None,
                },
                JobState::Compressed { attestation_hash } => JobStatusResponse {
                    job_id: job_id_str,
                    status: "proving".into(),
                    proof_path: None,
                    attestation_hash: Some(format!("0x{}", hex::encode(attestation_hash))),
                    tx_hash: None,
                    reason: None,
                },
                JobState::Failed { reason } => JobStatusResponse {
                    job_id: job_id_str,
                    status: "failed".into(),
                    proof_path: None,
                    attestation_hash: None,
                    tx_hash: None,
                    reason: Some(reason),
                },
            };
            HttpResponse::Ok().json(response)
        }
        Err(_) => HttpResponse::NotFound().json(err(format!("job not found: {job_id_str}"))),
    }
}

/// GET /v1/jobs/{id}/proof
#[get("/v1/jobs/{id}/proof")]
async fn get_proof(state: Data<AppState>, path: Path<String>) -> impl Responder {
    let job_id_str = path.into_inner();

    let job_state = match state.manager.status(&job_id_str).await {
        Ok(s) => s,
        Err(_) => {
            return HttpResponse::NotFound().json(err(format!("job not found: {job_id_str}")));
        }
    };

    match job_state {
        JobState::Compressed { attestation_hash } => HttpResponse::Ok().json(serde_json::json!({
            "job_id":           job_id_str,
            "status":           "compressed",
            "attestation_hash": format!("0x{}", hex::encode(attestation_hash)),
            "note": "proof is queued in the aggregator batch — \
                     Groth16 settlement happens per batch, not per job"
        })),
        JobState::Failed { reason } => {
            HttpResponse::UnprocessableEntity().json(err(format!("job failed: {reason}")))
        }
        _ => HttpResponse::Accepted().json(err("job not complete yet — poll /v1/jobs/{id} first")),
    }
}

/// GET /v1/proofs?page=1&limit=20
/// Returns a paginated list of all completed proof jobs, most recent first.
#[get("/v1/proofs")]
async fn list_proofs(
    state: Data<AppState>,
    query: actix_web::web::Query<ProofListQuery>,
) -> impl Responder {
    let limit = query.limit.clamp(1, 100);
    let offset = (query.page.saturating_sub(1)) * limit;

    match repo::list_jobs(&state.pool, limit, offset).await {
        Ok(jobs) => {
            let items: Vec<ProofListItem> = jobs
                .into_iter()
                .map(|j| ProofListItem {
                    job_id: j.id.to_string(),
                    status: j.status,
                    model_id: j.model_id.to_string(),
                    attestation_hash: j.attestation_hash,
                    tx_hash: j.tx_hash,
                    input_hash: j.input_hash,
                    completed_at: j.completed_at.map(|t| t.to_rfc3339()),
                    settled_at: j.settled_at.map(|t| t.to_rfc3339()),
                })
                .collect();

            HttpResponse::Ok().json(serde_json::json!({
                "page":   query.page,
                "limit":  limit,
                "proofs": items,
            }))
        }
        Err(e) => {
            error!("list_jobs failed: {e}");
            HttpResponse::InternalServerError().json(err("failed to fetch proofs"))
        }
    }
}

/// GET /v1/proofs/:attestation_hash
/// Fetch a single proof by its attestation hash.
/// Also accepts a job UUID as the path param for convenience.
#[get("/v1/proofs/{hash_or_id}")]
async fn get_proof_by_hash(state: Data<AppState>, path: Path<String>) -> impl Responder {
    let param = path.into_inner();

    // Try UUID first — cheaper DB lookup
    if let Ok(uuid) = Uuid::parse_str(&param) {
        if let Ok(job) = repo::find_job(&state.pool, uuid).await {
            return HttpResponse::Ok().json(ProofListItem {
                job_id: job.id.to_string(),
                status: job.status,
                model_id: job.model_id.to_string(),
                attestation_hash: job.attestation_hash,
                tx_hash: job.tx_hash,
                input_hash: job.input_hash,
                completed_at: job.completed_at.map(|t| t.to_rfc3339()),
                settled_at: job.settled_at.map(|t| t.to_rfc3339()),
            });
        }
    }

    // Fall back to attestation_hash lookup
    match repo::find_job_by_attestation_hash(&state.pool, &param).await {
        Ok(job) => HttpResponse::Ok().json(ProofListItem {
            job_id: job.id.to_string(),
            status: job.status,
            model_id: job.model_id.to_string(),
            attestation_hash: job.attestation_hash,
            tx_hash: job.tx_hash,
            input_hash: job.input_hash,
            completed_at: job.completed_at.map(|t| t.to_rfc3339()),
            settled_at: job.settled_at.map(|t| t.to_rfc3339()),
        }),
        Err(common::error::CommonError::NotFound(_)) => {
            HttpResponse::NotFound().json(err(format!("proof not found: {param}")))
        }
        Err(e) => {
            error!("find_job_by_attestation_hash failed: {e}");
            HttpResponse::InternalServerError().json(err("database error"))
        }
    }
}

/// GET /v1/account/:wallet
/// Returns vault balance, proof count, HSK spent for the given wallet.
/// Reads from DB (job records) + on-chain vault balance via VaultClient.
#[get("/v1/account/{wallet}")]
async fn get_account(state: Data<AppState>, path: Path<String>) -> impl Responder {
    let wallet = path.into_inner();

    // On-chain balance via VaultClient — this is the authoritative source.
    // FIX: vault.balance_of() now correctly accesses result._0 from the
    // Alloy sol! macro return struct.
    let vault_balance_wei = match &state.vault {
        Some(vault) => match vault.balance_of(&wallet).await {
            Ok(b) => b.to_string(),
            Err(e) => {
                warn!(wallet = %wallet, "vault balance_of failed: {e}");
                "0".into()
            }
        },
        None => "0".into(),
    };

    let balance_wei_u128: u128 = vault_balance_wei.parse().unwrap_or(0);
    let balance_hsk: f64 = balance_wei_u128 as f64 / 1e18;
    let proofs_remaining = (balance_hsk / 2.0).floor() as i64;

    // Per-wallet proof count and HSK spent from vault_events table.
    // FIX: use get_account_stats (queries vault_events) not count_settled_jobs.
    let (proof_count, hsk_spent_str) = match repo::get_account_stats(&state.pool, &wallet).await {
        Ok(s) => (s.proof_count, format!("{:.4}", s.proof_count as f64 * 2.0)),
        Err(e) => {
            warn!(wallet = %wallet, "get_account_stats failed: {e}");
            (0i64, "0.0000".into())
        }
    };

    HttpResponse::Ok().json(AccountResponse {
        wallet,
        vault_balance_wei,
        vault_balance_hsk: format!("{:.4}", balance_hsk),
        proof_count,
        hsk_spent: hsk_spent_str,
        proofs_remaining,
    })
}

// ── GET /v1/account/{wallet}/history ─────────────────────────────────────────
#[get("/v1/account/{wallet}/history")]
async fn get_account_history(
    state: Data<AppState>,
    path: Path<String>,
    query: Query<HistoryQuery>,
) -> impl Responder {
    let wallet = path.into_inner();
    let limit = query.limit.clamp(1, 100);
    let offset = (query.page.saturating_sub(1)) * limit;
    let operation = query.operation.as_deref();

    match repo::get_account_history(&state.pool, &wallet, limit, offset, operation).await {
        Ok(events) => {
            let entries: Vec<serde_json::Value> = events
                .into_iter()
                .map(|e| {
                    serde_json::json!({
                        "tx_hash":    e.tx_hash,
                        "operation":  e.operation,
                        "amount_wei": e.amount_wei,
                        "timestamp":  e.created_at.to_rfc3339(),
                        "job_id":     e.job_id.map(|id| id.to_string()),
                    })
                })
                .collect();
            HttpResponse::Ok().json(serde_json::json!({
                "page":    query.page,
                "limit":   limit,
                "entries": entries,
            }))
        }
        Err(e) => {
            error!("get_account_history failed: {e}");
            HttpResponse::InternalServerError().json(err("database error"))
        }
    }
}

#[post("/v1/account/{wallet}/events")]
async fn record_vault_event(
    state: Data<AppState>,
    path: Path<String>,
    Json(req): Json<RecordVaultEventRequest>,
) -> impl Responder {
    let wallet = path.into_inner();

    if req.tx_hash.trim().is_empty() || req.amount_wei.trim().is_empty() {
        return HttpResponse::BadRequest().json(err("tx_hash and amount_wei are required"));
    }

    if req.operation != "deposit" && req.operation != "deduct" {
        return HttpResponse::BadRequest().json(err("operation must be 'deposit' or 'deduct'"));
    }

    match repo::insert_vault_event(
        &state.pool,
        &wallet,
        &req.tx_hash,
        &req.operation,
        &req.amount_wei,
        None,
    )
    .await
    {
        Ok(()) => HttpResponse::Created().json(serde_json::json!({
            "ok": true,
            "wallet": wallet,
            "tx_hash": req.tx_hash,
        })),
        Err(e) => {
            error!(wallet = %wallet, "record_vault_event failed: {e}");
            HttpResponse::InternalServerError().json(err("database error"))
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(".env");
    let _ = dotenvy::from_path(&env_path);

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);

    // FIX 1: default to localhost:3000 (frontend dev server port)
    let client_url = std::env::var("CLIENT_URL").unwrap_or_else(|_| "http://localhost:3000".into());

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = common::db::build_pool(&database_url).expect("failed to build DB pool");
    common::db::run_migrations(&pool)
        .await
        .expect("failed to run DB migrations");

    info!("database connected and migrations applied");
    let pool = Arc::new(pool);

    let manager = ProverManager::new(prover_config());

    let (settler, settle_enabled) = match SettlerConfig::from_env() {
        Ok(cfg) => {
            info!(
                contract = %cfg.contract_address,
                rpc      = %cfg.rpc_url,
                "on-chain settlement enabled (HashKey testnet)"
            );
            (Arc::new(Settler::new(cfg)), true)
        }
        Err(e) => {
            warn!(
                "settlement disabled ({e}) — set SETTLER_RPC_URL, \
                 SETTLER_PRIVATE_KEY, INFERENCE_VERIFIER_ADDRESS"
            );
            let dummy_cfg = SettlerConfig {
                rpc_url: "http://localhost:8545".into(),
                private_key: "0x0000000000000000000000000000000000000000000000000000000000000001"
                    .into(),
                contract_address: "0x0000000000000000000000000000000000000000".into(),
                confirmations: 1,
                tx_timeout_secs: 120,
                eth_sepolia_inference_bridge: String::new(),
                rpc: String::new(),
                inference_verifier: String::new(),
                poll_interval_secs: 15,
                max_poll_attempts: 24,
                bridge_fee_wei: 0,
            };
            (Arc::new(Settler::new(dummy_cfg)), false)
        }
    };

    let ipfs_client = match PinataClient::from_env() {
        Ok(c) => {
            info!("IPFS client configured via Pinata");
            Some(Arc::new(c))
        }
        Err(e) => {
            warn!("IPFS not configured ({e}) — POST /v1/models will return 503");
            None
        }
    };

    let batch_cfg = BatchConfig::from_env();
    let batch_collector = BatchCollector::new(&batch_cfg)
        .await
        .expect("failed to initialise batch collector — check AGG_ELF_PATH");

    // Initialize the VaultClient if address is present
    let vault: Option<Arc<settler::vault::VaultClient>> = match std::env::var("VAULT_ADDRESS") {
        Ok(addr) => match SettlerConfig::from_env() {
            Ok(cfg) => {
                info!(vault_address = %addr, "VeilVault fee collection enabled");
                Some(Arc::new(settler::vault::VaultClient::new(cfg, addr)))
            }
            Err(e) => {
                warn!("VEIL_VAULT_ADDRESS set but settler config missing ({e}) — fees disabled");
                None
            }
        },
        Err(_) => {
            info!("VEIL_VAULT_ADDRESS not set — proofs are free");
            None
        }
    };

    info!("Veil Gateway starting on {host}:{port}");

    let state = Data::new(AppState {
        manager,
        settler,
        pool,
        settle_enabled,
        ipfs_client,
        batch_collector: Arc::clone(&batch_collector),
        vault,
    });

    // Flush timer
    {
        let collector = Arc::clone(&batch_collector);
        let settler = Arc::clone(&state.settler);
        let pool = Arc::clone(&state.pool);

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(collector.flush_secs));
            interval.tick().await; // skip first tick

            loop {
                interval.tick().await;

                let pending = collector.pending_count().await;
                if pending == 0 {
                    continue;
                }

                info!(pending, "flush timer fired — draining batch collector");
                let batch = collector.flush().await;
                if batch.is_empty() {
                    continue;
                }

                let elf = collector.agg_elf.clone();
                let settler = Arc::clone(&settler);
                let pool = Arc::clone(&pool);
                tokio::spawn(async move {
                    settle_batch(batch, &elf, &settler, &pool).await;
                });
            }
        });
    }

    HttpServer::new(move || {
        // FIX 2: allow both localhost:3000 and localhost:5173 so dev works
        // regardless of which port the frontend is on
        let cors = Cors::default()
            .allowed_origin(&client_url)
            .allowed_origin("http://localhost:5173")
            .allowed_origin("http://localhost:3000")
            .allowed_methods(vec!["GET", "POST", "OPTIONS"])
            .allowed_headers(vec![
                header::CONTENT_TYPE,
                header::AUTHORIZATION,
                header::ACCEPT,
            ])
            .supports_credentials()
            .max_age(3600);

        App::new()
            .app_data(state.clone())
            .wrap(cors)
            .wrap(actix_web::middleware::Logger::default())
            .service(healthz)
            .service(register_model)
            .service(submit_job)
            .service(get_job_status)
            .service(get_proof)
            .service(list_proofs)
            .service(get_proof_by_hash)
            .service(get_account)
            .service(get_account_history)
            .service(record_vault_event)
    })
    .bind((host.as_str(), port))?
    .run()
    .await
}
