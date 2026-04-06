use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Result};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::job::{JobState, ProofData};
use crate::proving;

#[derive(Debug, Clone)]
pub struct ProverConfig {
    pub proofs_dir: String,
    pub max_concurrent: usize,
    /// Wall-clock timeout for the full two-phase pipeline.
    /// Should be >= 180s (compressed ~60s + Groth16 ~120s).
    pub timeout_secs: u64,
    pub guest_elf_path: String,
    pub weights_path: String,
    /// How long Done/Failed entries live in memory before eviction.
    /// In-flight jobs are never evicted. Recommended: 3600s.
    pub job_ttl_secs: u64,
}

type JobMap = Arc<Mutex<HashMap<String, JobState>>>;
type ProofMap = Arc<Mutex<HashMap<String, ProofData>>>;
type EvictMap = Arc<Mutex<HashMap<String, std::time::Instant>>>;

pub struct ProverManager {
    config: ProverConfig,
    jobs: JobMap,
    proofs: ProofMap,
    evict: EvictMap,
    semaphore: Arc<tokio::sync::Semaphore>,
}

impl ProverManager {
    pub fn new(config: ProverConfig) -> Arc<Self> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent));

        let mgr = Arc::new(Self {
            config,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            proofs: Arc::new(Mutex::new(HashMap::new())),
            evict: Arc::new(Mutex::new(HashMap::new())),
            semaphore,
        });

        // Background TTL eviction — sweeps every 60s.
        // Only removes Done/Failed entries; in-flight jobs are never touched.
        {
            let jobs = Arc::clone(&mgr.jobs);
            let proofs = Arc::clone(&mgr.proofs);
            let evict = Arc::clone(&mgr.evict);
            let ttl = mgr.config.job_ttl_secs;
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    let now = std::time::Instant::now();
                    let mut ev = evict.lock().await;
                    let expired: Vec<String> = ev
                        .iter()
                        .filter(|(_, ts)| now.duration_since(**ts).as_secs() >= ttl)
                        .map(|(id, _)| id.clone())
                        .collect();
                    for id in &expired {
                        ev.remove(id);
                        jobs.lock().await.remove(id);
                        proofs.lock().await.remove(id);
                        info!(%id, "job evicted after TTL");
                    }
                }
            });
        }

        mgr
    }

    /// Submit a job. Returns the job_id immediately.
    ///
    /// Spawns a background task bounded by config.timeout_secs:
    ///   Phase 1 (compressed) -> JobState::Compressed  — attestation_hash available
    ///   Phase 2 (groth16)    -> JobState::Done        — proof written, ready for settlement
    ///
    /// On phase 2 failure the job is left in Compressed state (not Failed) so
    /// the gateway can still read the attestation_hash from phase 1.
    pub async fn submit(&self, input_data: Vec<Vec<f64>>) -> Result<String> {
        let job_id = Uuid::new_v4().to_string();
        let config = self.config.clone();
        let jobs = Arc::clone(&self.jobs);
        let proofs = Arc::clone(&self.proofs);
        let evict = Arc::clone(&self.evict);
        let sem = Arc::clone(&self.semaphore);
        let timeout = std::time::Duration::from_secs(self.config.timeout_secs);

        jobs.lock().await.insert(job_id.clone(), JobState::Queued);
        info!(%job_id, "job queued");

        let job_id_bg = job_id.clone();
        tokio::spawn(async move {
            let result = tokio::time::timeout(timeout, async {
                let _permit = sem.acquire().await.unwrap();

                jobs.lock().await.insert(job_id_bg.clone(), JobState::Running);
                info!(%job_id_bg, "job running — starting two-phase proving");

                // ── Phase 1: Compressed proof ─────────────────────────────────
                let phase1_result = proving::prove_compressed(
                    &config,
                    &job_id_bg,
                    input_data.clone(),
                )
                .await;

                let compressed_data = match phase1_result {
                    Ok(data) => data,
                    Err(e) => {
                        error!(%job_id_bg, "phase 1 (compressed) failed: {e}");
                        jobs.lock().await.insert(
                            job_id_bg.clone(),
                            JobState::Failed { reason: format!("compressed proof failed: {e}") },
                        );
                        evict.lock().await.insert(job_id_bg.clone(), std::time::Instant::now());
                        return;
                    }
                };

                let attestation_hash = compressed_data.attestation_hash;
                jobs.lock().await.insert(
                    job_id_bg.clone(),
                    JobState::Compressed { attestation_hash },
                );
                info!(
                    %job_id_bg,
                    attestation_hash = %hex::encode(attestation_hash),
                    "phase 1 complete — attestation_hash available"
                );

                // ── Phase 2: Groth16 ─────────────────────────────────────────
                let phase2_result = proving::prove_groth16(
                    &config,
                    &job_id_bg,
                    input_data,
                )
                .await;

                let groth16_data = match phase2_result {
                    Ok(data) => data,
                    Err(e) => {
                        error!(%job_id_bg, "phase 2 (groth16) failed: {e}");
                        // Leave job in Compressed state — attestation_hash remains
                        // valid and accessible to the gateway even when Groth16 fails.
                        warn!(%job_id_bg, "job left in Compressed state — attestation_hash intact");
                        evict.lock().await.insert(job_id_bg.clone(), std::time::Instant::now());
                        return;
                    }
                };

                // Sanity check: both phases must agree on attestation_hash.
                if groth16_data.attestation_hash != attestation_hash {
                    error!(
                        %job_id_bg,
                        compressed_hash = %hex::encode(attestation_hash),
                        groth16_hash    = %hex::encode(groth16_data.attestation_hash),
                        "attestation_hash mismatch between phases — ELF or weights changed"
                    );
                    jobs.lock().await.insert(
                        job_id_bg.clone(),
                        JobState::Failed {
                            reason: "attestation_hash mismatch between compressed and groth16 phases".into(),
                        },
                    );
                    evict.lock().await.insert(job_id_bg.clone(), std::time::Instant::now());
                    return;
                }

                let proof_path = format!("{}/{}.bin", config.proofs_dir, job_id_bg);
                if let Err(e) = tokio::fs::create_dir_all(&config.proofs_dir).await {
                    error!(%job_id_bg, "failed to create proofs_dir: {e}");
                    jobs.lock().await.insert(
                        job_id_bg.clone(),
                        JobState::Failed { reason: format!("proofs_dir creation failed: {e}") },
                    );
                    evict.lock().await.insert(job_id_bg.clone(), std::time::Instant::now());
                    return;
                }
                if let Err(e) = tokio::fs::write(&proof_path, &groth16_data.proof_bytes).await {
                    error!(%job_id_bg, "failed to write groth16 proof file: {e}");
                    jobs.lock().await.insert(
                        job_id_bg.clone(),
                        JobState::Failed { reason: format!("proof write failed: {e}") },
                    );
                    evict.lock().await.insert(job_id_bg.clone(), std::time::Instant::now());
                    return;
                }

                proofs.lock().await.insert(job_id_bg.clone(), groth16_data);
                jobs.lock().await.insert(
                    job_id_bg.clone(),
                    JobState::Done { proof_path: proof_path.clone() },
                );
                evict.lock().await.insert(job_id_bg.clone(), std::time::Instant::now());
                info!(%job_id_bg, %proof_path, "phase 2 complete — groth16 proof ready for settlement");
            })
            .await;

            if result.is_err() {
                error!(%job_id_bg, "job timed out");
                jobs.lock().await.insert(
                    job_id_bg.clone(),
                    JobState::Failed {
                        reason: "job timed out".into(),
                    },
                );
                evict
                    .lock()
                    .await
                    .insert(job_id_bg, std::time::Instant::now());
            }
        });

        Ok(job_id)
    }

    pub async fn status(&self, job_id: &str) -> Result<JobState> {
        self.jobs
            .lock()
            .await
            .get(job_id)
            .cloned()
            .ok_or_else(|| anyhow!("job not found: {job_id}"))
    }

    pub async fn read_proof(&self, job_id: &str) -> Result<Vec<u8>> {
        match self.status(job_id).await? {
            JobState::Done { proof_path } => tokio::fs::read(&proof_path)
                .await
                .map_err(|e| anyhow!("failed to read proof at {proof_path}: {e}")),
            JobState::Compressed { .. } => Err(anyhow!(
                "groth16 proof not yet ready — job is in Compressed state"
            )),
            _ => Err(anyhow!("proof not available — job is not in Done state")),
        }
    }

    pub async fn proof_data(&self, job_id: &str) -> Result<ProofData> {
        self.proofs
            .lock()
            .await
            .get(job_id)
            .cloned()
            .ok_or_else(|| {
                anyhow!("proof data not found for job '{job_id}' — job may not be Done yet")
            })
    }

    pub async fn attestation_hash(&self, job_id: &str) -> Result<[u8; 32]> {
        match self.status(job_id).await? {
            JobState::Compressed { attestation_hash } => Ok(attestation_hash),
            JobState::Done { .. } => self
                .proofs
                .lock()
                .await
                .get(job_id)
                .map(|pd| pd.attestation_hash)
                .ok_or_else(|| anyhow!("proof data missing for Done job '{job_id}'")),
            JobState::Queued | JobState::Running => Err(anyhow!(
                "attestation_hash not yet available — job is still proving"
            )),
            JobState::Failed { reason } => Err(anyhow!(
                "job failed before attestation_hash was computed: {reason}"
            )),
        }
    }
}
