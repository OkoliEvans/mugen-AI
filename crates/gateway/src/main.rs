//! Elenxis Gateway — Actix-Web HTTP API
//!
//! Endpoints:
//!   POST   /v1/models          — register a model (IPFS + on-chain)
//!   POST   /v1/jobs            — submit inference job
//!   GET    /v1/jobs/{id}       — poll job status
//!   GET    /v1/jobs/{id}/proof — fetch proof bytes (hex)
//!   GET    /healthz            — liveness check
//!
//! Phase 2: all job lifecycle events are persisted to Postgres via the
//! `common` crate. In-memory prover-manager state is the source of truth
//! for in-flight jobs; Postgres is the durable record for completed jobs.

use std::sync::Arc;

use actix_web::{
    get, post,
    web::{Data, Json, Path},
    App, HttpResponse, HttpServer, Responder,
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
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

// ── Config ────────────────────────────────────────────────────────────────────

fn prover_config() -> ProverConfig {
    ProverConfig {
        python_bin: std::env::var("PYTHON_BIN").unwrap_or_else(|_| "python3".into()),
        worker_script: std::env::var("WORKER_SCRIPT").unwrap_or_else(|_| "prover/worker.py".into()),
        artifacts_dir: std::env::var("ARTIFACTS_DIR").unwrap_or_else(|_| "prover/artifacts".into()),
        max_concurrent: std::env::var("MAX_CONCURRENT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2),
        timeout_secs: std::env::var("TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120),
    }
}

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    manager:        Arc<ProverManager>,
    settler:        Arc<Settler>,
    pool:           Arc<DbPool>,
    settle_enabled: bool,
    /// None when PINATA_JWT is not set — model registration endpoint will
    /// return 503 until the gateway is configured with IPFS credentials.
    ipfs_client:    Option<Arc<PinataClient>>,
}

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SubmitRequest {
    input_data: Vec<Vec<f64>>,
    #[serde(default = "default_model_id")]
    model_id: String,
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

/// POST /v1/models request body.
///
/// `artifact_b64` is the base64-encoded ONNX model file.
/// `input_shape`  is the model's expected input dimensions, e.g. [1, 4].
#[derive(Debug, Deserialize)]
struct RegisterModelRequest {
    name:         String,
    version:      String,
    artifact_b64: String,
    input_shape:  Vec<u64>,
}

#[derive(Debug, Serialize)]
struct RegisterModelResponse {
    /// Postgres UUID for this model row.
    model_id:          String,
    /// keccak256(name + version) — the on-chain registry key.
    on_chain_model_id: String,
    /// IPFS CID of the pinned model artifact.
    ipfs_cid:          String,
    /// Public gateway URL for the pinned artifact.
    gateway_url:       String,
    /// Transaction hash of the on-chain registerModel() call.
    on_chain_hash:     String,
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

// ── Model upsert — lazy registration ─────────────────────────────────────────

/// Look up a model by name (treating model_id string as name).
/// If not found, insert a placeholder record with derived input_shape.
/// If found but input_shape is still empty (legacy placeholder), patch it.
///
/// NOTE: ipfs_cid and on_chain_hash remain "pending" until the full
/// POST /v1/models registration flow is implemented (IPFS pin + on-chain
/// registerModel() call). This is intentional for the dev phase.
async fn upsert_model(
    pool: &DbPool,
    name: &str,
    input_data: &[Vec<f64>],
) -> Result<(Uuid, String), String> {
    // Derive shape [rows, cols] from the actual input
    let shape = serde_json::json!([
        input_data.len(),
        input_data.first().map(|r| r.len()).unwrap_or(0)
    ]);

    match repo::find_model_by_name(pool, name).await {
        Ok(m) => {
            // Patch shape if this row was inserted as a placeholder before
            // this fix was deployed (i.e. shape is still empty array [])
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

    // Not found — insert with derived shape
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

/// Compute SHA-256 of the raw input bytes, returned as hex string.
fn hash_input(input_data: &[Vec<f64>]) -> String {
    let bytes = serde_json::to_vec(input_data).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    hex::encode(digest)
}

// ── Settlement background task ────────────────────────────────────────────────

/// Spawned after job submission. Tracks the job through its full lifecycle
/// and persists every state transition to Postgres.
fn spawn_settler(
    state: AppState,
    job_id: String,
    db_job_id: Uuid,
    model_name: String,
    model_version: String,
    input_data: Vec<Vec<f64>>,
) {
    tokio::spawn(async move {
        // Mark job as running in Postgres
        let _ = repo::update_job(
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
            },
        )
        .await;

        // Poll prover-manager until Done or Failed
        let proof_path = {
            let mut attempts = 0u32;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                match state.manager.status(&job_id).await {
                    Ok(JobState::Done { proof_path }) => break proof_path,
                    Ok(JobState::Failed { reason }) => {
                        warn!(%job_id, %reason, "job failed — persisting to DB");
                        let _ = repo::update_job(
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
                            },
                        )
                        .await;
                        return;
                    }
                    Ok(_) => {
                        attempts += 1;
                        if attempts > 120 {
                            warn!(%job_id, "settler timed out waiting for proof");
                            let _ = repo::update_job(
                                &state.pool,
                                db_job_id,
                                JobUpdate {
                                    status: "failed".into(),
                                    proof_path: None,
                                    error: Some("prover timeout".into()),
                                    started_at: None,
                                    completed_at: Some(Utc::now()),
                                    settled_at: None,
                                    tx_hash: None,
                                    batch_id: None,
                                },
                            )
                            .await;
                            return;
                        }
                    }
                    Err(e) => {
                        error!(%job_id, "settler: status error: {e}");
                        return;
                    }
                }
            }
        };

        // Mark proof done in Postgres
        let _ = repo::update_job(
            &state.pool,
            db_job_id,
            JobUpdate {
                status: "done".into(),
                proof_path: Some(proof_path.clone()),
                error: None,
                started_at: None,
                completed_at: Some(Utc::now()),
                settled_at: None,
                tx_hash: None,
                batch_id: None,
            },
        )
        .await;

        info!(%job_id, %proof_path, "proof ready — submitting on-chain");

        if !state.settle_enabled {
            return;
        }

        let input_bytes = serde_json::to_vec(&input_data).unwrap_or_default();
        let output_bytes = job_id.as_bytes().to_vec();

        match state
            .settler
            .submit(&proof_path, &model_name, &model_version, &input_bytes, &output_bytes)
            .await
        {
            Ok(tx_hash) if tx_hash == "already-verified" => {
                warn!(%job_id, "proof already verified on-chain");
                let _ = repo::update_job(
                    &state.pool,
                    db_job_id,
                    JobUpdate {
                        status: "settled".into(),
                        proof_path: None,
                        error: None,
                        started_at: None,
                        completed_at: None,
                        settled_at: Some(Utc::now()),
                        tx_hash: Some("already-verified".into()),
                        batch_id: None,
                    },
                )
                .await;
            }
            Ok(tx_hash) => {
                info!(%job_id, %tx_hash, "proof settled on-chain — persisting tx_hash");
                let _ = repo::update_job(
                    &state.pool,
                    db_job_id,
                    JobUpdate {
                        status: "settled".into(),
                        proof_path: None,
                        error: None,
                        started_at: None,
                        completed_at: None,
                        settled_at: Some(Utc::now()),
                        tx_hash: Some(tx_hash),
                        batch_id: None,
                    },
                )
                .await;
            }
            Err(e) => {
                error!(%job_id, "settlement failed: {e}");
                let _ = repo::update_job(
                    &state.pool,
                    db_job_id,
                    JobUpdate {
                        status: "failed".into(),
                        proof_path: None,
                        error: Some(format!("settlement: {e}")),
                        started_at: None,
                        completed_at: None,
                        settled_at: None,
                        tx_hash: None,
                        batch_id: None,
                    },
                )
                .await;
            }
        }
    });
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /v1/models
///
/// Full model registration pipeline:
///   1. Decode base64 artifact bytes
///   2. Pin artifact to IPFS via Pinata → CID
///   3. Call settler.register_model() → on-chain tx hash
///   4. Upsert model row in Postgres with CID + tx hash
///
/// Returns 503 if the gateway is not configured with IPFS credentials
/// (PINATA_JWT not set). Returns 409 if the model is already registered.
#[post("/v1/models")]
async fn register_model(
    state: Data<AppState>,
    Json(req): Json<RegisterModelRequest>,
) -> impl Responder {
    // ── Validate input ────────────────────────────────────────────────────────
    if req.name.trim().is_empty() || req.version.trim().is_empty() {
        return HttpResponse::BadRequest().json(err("name and version must not be empty"));
    }
    if req.artifact_b64.is_empty() {
        return HttpResponse::BadRequest().json(err("artifact_b64 must not be empty"));
    }
    if req.input_shape.is_empty() {
        return HttpResponse::BadRequest().json(err("input_shape must not be empty"));
    }

    // ── IPFS client must be configured ───────────────────────────────────────
    let ipfs = match state.ipfs_client.as_ref() {
        Some(c) => c,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(err("IPFS not configured — set PINATA_JWT and PINATA_GATEWAY_URL"));
        }
    };

    // ── Reject if already registered ─────────────────────────────────────────
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

    // ── 1. Decode artifact ────────────────────────────────────────────────────
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

    // ── 2. Pin artifact to IPFS ───────────────────────────────────────────────
    let filename = format!("{}_{}.onnx", req.name, req.version.replace('.', "_"));
    let meta = PinMeta {
        name: format!("{}@{}", req.name, req.version),
        keyvalues: Some(serde_json::json!({
            "model_name":    req.name,
            "model_version": req.version,
        })),
    };

    let ipfs_cid = match ipfs.pin_bytes(artifact_bytes, &filename, Some(meta)).await {
        Ok(cid) => cid,
        Err(e) => {
            error!(model_name = %req.name, "IPFS pin failed: {e}");
            return HttpResponse::BadGateway()
                .json(err(format!("IPFS pin failed: {e}")));
        }
    };

    let gateway_url = ipfs.gateway_url(&ipfs_cid);
    info!(model_name = %req.name, %ipfs_cid, "artifact pinned to IPFS");

    // ── 3. Register on-chain ──────────────────────────────────────────────────
    if !state.settle_enabled {
        warn!(
            model_name = %req.name,
            "settlement disabled — skipping on-chain registration"
        );
        // Still persist the CID so the record isn't stuck at "pending"
        let on_chain_hash = upsert_registered_model(
            &state.pool,
            &req.name,
            &req.version,
            &req.input_shape,
            &ipfs_cid,
            "settlement-disabled",
        )
        .await;

        return match on_chain_hash {
            Ok((model_id, on_chain_model_id)) => HttpResponse::Created().json(RegisterModelResponse {
                model_id:          model_id.to_string(),
                on_chain_model_id,
                ipfs_cid,
                gateway_url,
                on_chain_hash:     "settlement-disabled".into(),
            }),
            Err(e) => HttpResponse::InternalServerError().json(err(e)),
        };
    }

    let on_chain_hash = match state
        .settler
        .register_model(&req.name, &req.version, &ipfs_cid, &req.input_shape)
        .await
    {
        Ok(tx_hash) => tx_hash,
        Err(e) => {
            error!(model_name = %req.name, "on-chain registration failed: {e}");
            return HttpResponse::BadGateway()
                .json(err(format!("on-chain registration failed: {e}")));
        }
    };

    info!(model_name = %req.name, %on_chain_hash, "model registered on-chain");

    // ── 4. Persist to Postgres ────────────────────────────────────────────────
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
            // On-chain tx already confirmed — return partial success with the CID
            // and tx hash so the caller can manually reconcile the DB if needed.
            HttpResponse::MultiStatus().json(serde_json::json!({
                "warning":       "model registered on-chain but DB update failed",
                "ipfs_cid":      ipfs_cid,
                "on_chain_hash": on_chain_hash,
                "error":         e,
            }))
        }
    }
}

/// Upsert the model row in Postgres with the confirmed IPFS CID and on-chain hash.
/// Returns (postgres_uuid, on_chain_model_id_hex).
async fn upsert_registered_model(
    pool:          &DbPool,
    name:          &str,
    version:       &str,
    input_shape:   &[u64],
    ipfs_cid:      &str,
    on_chain_hash: &str,
) -> Result<(Uuid, String), String> {
    let shape = serde_json::json!(input_shape);

    // Derive the same modelId the contract uses: keccak256(name + version)
    use alloy::primitives::keccak256;
    let mut model_id_input = Vec::with_capacity(name.len() + version.len());
    model_id_input.extend_from_slice(name.as_bytes());
    model_id_input.extend_from_slice(version.as_bytes());
    let on_chain_model_id = format!("0x{}", hex::encode(keccak256(&model_id_input)));

    match repo::find_model_by_name(pool, name).await {
        Ok(m) => {
            // Row exists (lazy placeholder) — update all fields
            repo::update_model_registration(pool, m.id, ipfs_cid.to_string(), on_chain_hash.to_string())
                .await
                .map_err(|e| e.to_string())?;

            // Also patch shape if it was never set
            if m.input_shape == serde_json::json!([]) {
                let _ = repo::update_model_shape(pool, m.id, shape).await;
            }

            Ok((m.id, on_chain_model_id))
        }
        Err(common::error::CommonError::NotFound(_)) => {
            // No placeholder row — insert fresh
            let id = Uuid::new_v4();
            let new = NewModel {
                id,
                name:          name.to_string(),
                version:       version.to_string(),
                ipfs_cid:      ipfs_cid.to_string(),
                input_shape:   shape,
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

    let model_name = req.model_id.clone();
    let input_data = req.input_data.clone();

    // 1. Resolve model UUID — upsert with derived input_shape
    let (model_uuid, model_version) = match upsert_model(&state.pool, &model_name, &input_data).await {
        Ok(v) => v,
        Err(e) => {
            error!("model upsert failed: {e}");
            return HttpResponse::InternalServerError()
                .json(err(format!("model registry error: {e}")));
        }
    };

    // 2. Submit to in-memory prover-manager
    let job_id = match state.manager.submit(req.input_data).await {
        Ok(id) => id,
        Err(e) => {
            error!("submit failed: {e}");
            return HttpResponse::InternalServerError().json(err(e.to_string()));
        }
    };

    // 3. Parse job_id as UUID (prover-manager uses uuid::Uuid internally)
    let db_job_id = match Uuid::parse_str(&job_id) {
        Ok(id) => id,
        Err(e) => {
            error!("invalid job_id UUID: {e}");
            return HttpResponse::InternalServerError().json(err("internal id error"));
        }
    };

    // 4. Persist job record to Postgres
    let input_hash = hash_input(&input_data);
    let new_job = NewJob {
        id: db_job_id,
        model_id: model_uuid,
        status: "queued".into(),
        input_hash,
    };

    if let Err(e) = repo::insert_job(&state.pool, new_job).await {
        error!(%job_id, "failed to persist job to DB: {e}");
        // Non-fatal — job still runs, just not persisted
    }

    info!(%job_id, %model_name, "job submitted and persisted");

    // 5. Spawn background settler + DB updater
    spawn_settler(
        state.get_ref().clone(),
        job_id.clone(),
        db_job_id,
        model_name,
        model_version,
        input_data,
    );

    HttpResponse::Accepted().json(SubmitResponse {
        job_id: job_id.clone(),
        status: "queued",
    })
}

/// GET /v1/jobs/{id}
/// Reads from Postgres for settled/failed jobs, falls back to in-memory
/// for in-flight jobs (queued/running).
#[get("/v1/jobs/{id}")]
async fn get_job_status(state: Data<AppState>, path: Path<String>) -> impl Responder {
    let job_id_str = path.into_inner();

    let db_id = match Uuid::parse_str(&job_id_str) {
        Ok(id) => id,
        Err(_) => {
            return HttpResponse::BadRequest().json(err("invalid job id format"));
        }
    };

    // Try Postgres first for terminal states
    if let Ok(job) = repo::find_job(&state.pool, db_id).await {
        if job.status == "settled" || job.status == "failed" || job.status == "done" {
            return HttpResponse::Ok().json(JobStatusResponse {
                job_id: job_id_str,
                status: job.status,
                proof_path: job.proof_path,
                tx_hash: job.tx_hash,
                reason: job.error,
            });
        }
    }

    // Fall back to in-memory for queued/running
    match state.manager.status(&job_id_str).await {
        Ok(job_state) => {
            let response = match job_state {
                JobState::Queued => JobStatusResponse {
                    job_id: job_id_str,
                    status: "queued".into(),
                    proof_path: None,
                    tx_hash: None,
                    reason: None,
                },
                JobState::Running => JobStatusResponse {
                    job_id: job_id_str,
                    status: "running".into(),
                    proof_path: None,
                    tx_hash: None,
                    reason: None,
                },
                JobState::Done { proof_path } => JobStatusResponse {
                    job_id: job_id_str,
                    status: "done".into(),
                    proof_path: Some(proof_path),
                    tx_hash: None,
                    reason: None,
                },
                JobState::Failed { reason } => JobStatusResponse {
                    job_id: job_id_str,
                    status: "failed".into(),
                    proof_path: None,
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
        JobState::Done { .. } => {}
        JobState::Failed { reason } => {
            return HttpResponse::UnprocessableEntity().json(err(format!("job failed: {reason}")));
        }
        _ => {
            return HttpResponse::Accepted()
                .json(err("job not complete yet — poll /v1/jobs/{id} first"));
        }
    }

    match state.manager.read_proof(&job_id_str).await {
        Ok(bytes) => {
            let size_bytes = bytes.len();
            let proof_hex = hex::encode(&bytes);
            HttpResponse::Ok().json(ProofResponse {
                job_id: job_id_str,
                proof_hex,
                size_bytes,
            })
        }
        Err(e) => {
            error!("read_proof failed: {e}");
            HttpResponse::InternalServerError().json(err(e.to_string()))
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load .env from workspace root
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

    // ── Database pool ─────────────────────────────────────────────────────────
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = common::db::build_pool(&database_url).expect("failed to build DB pool");

    // Run pending Diesel migrations on startup
    common::db::run_migrations(&pool)
        .await
        .expect("failed to run DB migrations");

    info!("database connected and migrations applied");

    let pool = Arc::new(pool);

    // ── Prover manager ────────────────────────────────────────────────────────
    let manager = Arc::new(ProverManager::new(prover_config()));

    // ── Settler ───────────────────────────────────────────────────────────────
    let (settler, settle_enabled) = match SettlerConfig::from_env() {
        Ok(cfg) => {
            info!("on-chain settlement enabled → {}", cfg.contract_address);
            (Arc::new(Settler::new(cfg)), true)
        }
        Err(e) => {
            warn!("settlement disabled ({e}) — set SETTLER_* env vars to enable");
            let dummy_cfg = SettlerConfig {
                rpc_url: "http://localhost:8545".into(),
                private_key: "0x0000000000000000000000000000000000000000000000000000000000000001"
                    .into(),
                contract_address: "0x0000000000000000000000000000000000000000".into(),
                confirmations: 1,
                tx_timeout_secs: 120,
            };
            (Arc::new(Settler::new(dummy_cfg)), false)
        }
    };

    // ── IPFS client ───────────────────────────────────────────────────────────
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

    info!("Elenxis Gateway starting on {host}:{port}");

    let state = Data::new(AppState {
        manager,
        settler,
        pool,
        settle_enabled,
        ipfs_client,
    });

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .wrap(actix_web::middleware::Logger::default())
            .service(healthz)
            .service(register_model)
            .service(submit_job)
            .service(get_job_status)
            .service(get_proof)
    })
    .bind((host.as_str(), port))?
    .run()
    .await
}