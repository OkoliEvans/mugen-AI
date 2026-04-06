// gateway/src/main.rs

//! Veil Gateway — Actix-Web HTTP API
//!
//! Endpoints:
//!   POST   /v1/models          — register a model (IPFS + on-chain)
//!   POST   /v1/jobs            — submit inference job
//!   GET    /v1/jobs/{id}       — poll job status
//!   GET    /v1/jobs/{id}/proof — fetch proof bytes (hex)
//!   GET    /healthz            — liveness check
//!
//! Job lifecycle:
//!   queued → running → proving → done → settled
//!
//!   proving = phase 1 (compressed STARK) complete, attestation_hash available
//!   done    = phase 2 (groth16 SNARK) complete, proof ready for on-chain settlement
//!
//! In-memory prover-manager is the source of truth for in-flight jobs.
//! Postgres is the durable record for all completed jobs.

use std::sync::Arc;

use actix_cors::Cors;
use actix_web::{
    get,
    http::header,
    post,
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

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    manager: Arc<ProverManager>,
    settler: Arc<Settler>,
    pool: Arc<DbPool>,
    settle_enabled: bool,
    /// None when PINATA_JWT is not set — model registration will return 503.
    ipfs_client: Option<Arc<PinataClient>>,
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
    /// Postgres UUID for this model row.
    model_id: String,
    /// keccak256(name + version) — the on-chain registry key.
    on_chain_model_id: String,
    /// IPFS CID of the pinned model artifact.
    ipfs_cid: String,
    /// Public gateway URL for the pinned artifact.
    gateway_url: String,
    /// Transaction hash of the on-chain registerModel() call.
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

// ── Model upsert — lazy registration ─────────────────────────────────────────

/// Look up a model by name. If not found, insert a placeholder record.
/// If found but input_shape is still empty (legacy placeholder), patch it.
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

/// Compute SHA-256 of the raw input bytes, returned as hex string.
fn hash_input(input_data: &[Vec<f64>]) -> String {
    let bytes = serde_json::to_vec(input_data).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    hex::encode(digest)
}

// ── Settlement background task ────────────────────────────────────────────────

/// Spawned after job submission. Tracks the job through both proving phases
/// and persists every state transition to Postgres.
///
/// Two-phase lifecycle:
///   Phase 1 (Compressed ~30-60s):
///     attestation_hash available → persisted immediately with status "proving"
///   Phase 2 (Groth16 ~90-120s):
///     proof_path written → status "done" → on-chain settlement triggered
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
                attestation_hash: None,
            },
        )
        .await;

        // ── Poll through both proving phases ──────────────────────────────────
        //
        // Phase 1 (Compressed) → JobState::Compressed
        //   Persist attestation_hash immediately with status "proving".
        //   The API can return attestation_hash to callers without waiting
        //   for Groth16 to complete.
        //
        // Phase 2 (Groth16) → JobState::Done
        //   Proof file written to disk. Trigger on-chain settlement.
        //
        // Poll at 500ms intervals. 480 attempts = 240s budget.
        // Covers compressed (~60s) + groth16 (~120s) + network headroom.
        let proof_path = {
            let mut attempts = 0u32;
            let max_attempts = 480u32;
            let mut attestation_persisted = false;

            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                attempts += 1;

                match state.manager.status(&job_id).await {
                    // Phase 1 complete — persist attestation_hash, keep polling
                    Ok(JobState::Compressed { attestation_hash }) => {
                        if !attestation_persisted {
                            let hash_hex = format!("0x{}", hex::encode(attestation_hash));
                            info!(%job_id, attestation_hash = %hash_hex, "phase 1 complete — persisting attestation_hash");

                            let _ = repo::update_job(
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
                                    attestation_hash: Some(hash_hex),
                                },
                            )
                            .await;

                            attestation_persisted = true;
                        }
                        // Continue polling — waiting for Groth16 (phase 2)
                    }

                    // Phase 2 complete — break with proof_path for settlement
                    Ok(JobState::Done { proof_path }) => break proof_path,

                    // Terminal failure at any phase
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
                                attestation_hash: None,
                            },
                        )
                        .await;
                        return;
                    }

                    // Still Queued or Running — check timeout
                    Ok(_) => {
                        if attempts >= max_attempts {
                            warn!(%job_id, "settler timed out waiting for Groth16 proof");
                            let _ = repo::update_job(
                                &state.pool,
                                db_job_id,
                                JobUpdate {
                                    status: "failed".into(),
                                    proof_path: None,
                                    error: Some(
                                        "prover timeout — exceeded max poll attempts".into(),
                                    ),
                                    started_at: None,
                                    completed_at: Some(Utc::now()),
                                    settled_at: None,
                                    tx_hash: None,
                                    batch_id: None,
                                    attestation_hash: None,
                                },
                            )
                            .await;
                            return;
                        }
                    }

                    Err(e) => {
                        error!(%job_id, "settler: status poll error: {e}");
                        return;
                    }
                }
            }
        };

        // Both phases complete. Fetch attestation_hash from Groth16 ProofData
        // (canonical source after phase 2 — cross-checks phase 1 value).
        let attestation_hash_hex = match state.manager.proof_data(&job_id).await {
            Ok(pd) => {
                let hex = format!("0x{}", hex::encode(pd.attestation_hash));
                info!(%job_id, attestation_hash = %hex, "groth16 attestation_hash confirmed");
                Some(hex)
            }
            Err(e) => {
                warn!(%job_id, "could not fetch proof_data after Done: {e}");
                None
            }
        };

        // Persist Done — proof_path + confirmed attestation_hash
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
                attestation_hash: attestation_hash_hex,
            },
        )
        .await;

        info!(%job_id, %proof_path, "groth16 proof ready — submitting on-chain");

        if !state.settle_enabled {
            return;
        }

        // Mock proofs have empty bytes and will always revert on-chain.
        // Only submit real network proofs.
        if std::env::var("SP1_PROVER").as_deref() == Ok("mock") {
            warn!(%job_id, "SP1_PROVER=mock — skipping settlement (mock proof has no real bytes)");
            return;
        }

        let input_bytes = serde_json::to_vec(&input_data).unwrap_or_default();
        let output_bytes = job_id.as_bytes().to_vec();

        match state
            .settler
            .submit(
                &proof_path,
                &model_name,
                &model_version,
                &input_bytes,
                &output_bytes,
            )
            .await
        {
            Ok(tx_hash) if tx_hash == "already-verified" => {
                warn!(%job_id, "proof already verified on HashKey — skipping");
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
                        attestation_hash: None,
                    },
                )
                .await;
            }
            Ok(tx_hash) => {
                info!(%job_id, %tx_hash, "proof settled on HashKey testnet");
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
                        attestation_hash: None,
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
                        attestation_hash: None,
                    },
                )
                .await;
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
                "ipfs_cid":      ipfs_cid,
                "on_chain_hash": on_chain_hash,
                "error":         e,
            }))
        }
    }
}

/// Upsert the model row with confirmed IPFS CID and on-chain hash.
/// Returns (postgres_uuid, on_chain_model_id_hex).
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

    let model_name = req.model_id.clone();
    let input_data = req.input_data.clone();

    // 1. Resolve model UUID
    let (model_uuid, model_version) =
        match upsert_model(&state.pool, &model_name, &input_data).await {
            Ok(v) => v,
            Err(e) => {
                error!("model upsert failed: {e}");
                return HttpResponse::InternalServerError()
                    .json(err(format!("model registry error: {e}")));
            }
        };

    // 2. Submit to in-memory prover-manager
    let job_id = match state.manager.submit(input_data.clone()).await {
        Ok(id) => id,
        Err(e) => {
            error!("submit failed: {e}");
            return HttpResponse::InternalServerError().json(err(e.to_string()));
        }
    };

    // 3. Parse job_id as UUID
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
    }

    info!(%job_id, %model_name, "job submitted and persisted");

    // 5. Spawn background settler — tracks both proving phases
    spawn_settler(
        state.get_ref().clone(),
        job_id.clone(),
        db_job_id,
        model_name,
        model_version,
        input_data,
    );

    HttpResponse::Accepted().json(SubmitResponse {
        job_id,
        status: "queued",
    })
}

/// GET /v1/jobs/{id}
/// Always prefers Postgres — in-memory fallback only when Postgres has no record yet.
/// Returns attestation_hash as soon as phase 1 completes (status = "proving").
#[get("/v1/jobs/{id}")]
async fn get_job_status(state: Data<AppState>, path: Path<String>) -> impl Responder {
    let job_id_str = path.into_inner();

    let db_id = match Uuid::parse_str(&job_id_str) {
        Ok(id) => id,
        Err(_) => {
            return HttpResponse::BadRequest().json(err("invalid job id format"));
        }
    };

    // Postgres is the durable source — includes attestation_hash once "proving"
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

    // In-memory fallback — job not yet flushed to Postgres
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
                // Phase 1 done — attestation_hash available, Groth16 in progress
                JobState::Compressed { attestation_hash } => JobStatusResponse {
                    job_id: job_id_str,
                    status: "proving".into(),
                    proof_path: None,
                    attestation_hash: Some(format!("0x{}", hex::encode(attestation_hash))),
                    tx_hash: None,
                    reason: None,
                },
                // Phase 2 done — Groth16 ready, settlement pending
                JobState::Done { proof_path } => {
                    let attestation_hash = state
                        .manager
                        .proof_data(&job_id_str)
                        .await
                        .ok()
                        .map(|pd| format!("0x{}", hex::encode(pd.attestation_hash)));
                    JobStatusResponse {
                        job_id: job_id_str,
                        status: "done".into(),
                        proof_path: Some(proof_path),
                        attestation_hash,
                        tx_hash: None,
                        reason: None,
                    }
                }
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
/// Only available after phase 2 (Groth16) completes — status = "done".
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
        JobState::Compressed { .. } => {
            return HttpResponse::Accepted().json(err(
                "groth16 proof not yet ready — job is proving (phase 2 in progress)",
            ));
        }
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

    let client_url = std::env::var("CLIENT_URL").unwrap_or_else(|_| "http://localhost:5173".into());

    // ── Database ──────────────────────────────────────────────────────────────
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = common::db::build_pool(&database_url).expect("failed to build DB pool");
    common::db::run_migrations(&pool)
        .await
        .expect("failed to run DB migrations");

    info!("database connected and migrations applied");
    let pool = Arc::new(pool);

    // ── Prover manager — pre-warms proving key at startup ─────────────────────
    // ProverManager::new() calls proving::setup() which reads the ELF from disk
    // and runs client.setup() once. All subsequent jobs reuse the cached key.
    // Panics if ELF or weights are missing — fail fast at startup.
    let manager = ProverManager::new(prover_config());

    // ── Settler — HashKey testnet ─────────────────────────────────────────────
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
            warn!("settlement disabled ({e}) — set SETTLER_RPC_URL, SETTLER_PRIVATE_KEY, INFERENCE_VERIFIER_ADDRESS");
            let dummy_cfg = SettlerConfig {
                rpc_url: "http://localhost:8545".into(),
                private_key: "0x0000000000000000000000000000000000000000000000000000000000000001"
                    .into(),
                contract_address: "0x0000000000000000000000000000000000000000".into(),
                confirmations: 1,
                tx_timeout_secs: 120,
                // Kept for struct compat — unused in HashKey path
                eth_sepolia_inference_bridge: String::new(),
                starknet_rpc: String::new(),
                starknet_inference_verifier: String::new(),
                starknet_poll_interval_secs: 15,
                starknet_max_poll_attempts: 24,
                starknet_bridge_fee_wei: 0,
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

    info!("Veil Gateway starting on {host}:{port}");

    let state = Data::new(AppState {
        manager,
        settler,
        pool,
        settle_enabled,
        ipfs_client,
    });

    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin(&client_url)
            .allowed_methods(vec!["GET", "POST", "OPTIONS"])
            .allowed_headers(vec![header::CONTENT_TYPE, header::AUTHORIZATION])
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
    })
    .bind((host.as_str(), port))?
    .run()
    .await
}
