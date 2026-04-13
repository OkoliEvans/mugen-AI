// crates/prover-manager/src/manager.rs

use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Result};
use tokio::sync::Mutex;
use tracing::{error, info};
use uuid::Uuid;

use crate::job::{CompressedData, JobState};
use crate::proving;

#[derive(Debug, Clone)]
pub struct ProverConfig {
    pub proofs_dir: String,
    pub max_concurrent: usize,
    /// Wall-clock timeout for compressed proving only.
    /// ~60–90s on the Succinct Prover Network. No Groth16 per job.
    pub timeout_secs: u64,
    pub guest_elf_path: String,
    pub weights_path: String,
    /// How long Compressed/Failed entries live in memory before eviction.
    /// Must be >= the aggregator's collection window (default: 3600s).
    pub job_ttl_secs: u64,
}

type JobMap        = Arc<Mutex<HashMap<String, JobState>>>;
type CompressedMap = Arc<Mutex<HashMap<String, CompressedData>>>;
type EvictMap      = Arc<Mutex<HashMap<String, std::time::Instant>>>;

pub struct ProverManager {
    config: ProverConfig,
    jobs: JobMap,
    /// Stores phase 1 output — live SP1ProofWithPublicValues + vk.
    /// Populated when Compressed state is set.
    /// Consumed by the aggregator batch collector.
    /// Evicted on the same TTL as jobs.
    compressed_proofs: CompressedMap,
    evict: EvictMap,
    semaphore: Arc<tokio::sync::Semaphore>,
}

impl ProverManager {
    pub fn new(config: ProverConfig) -> Arc<Self> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent));

        let mgr = Arc::new(Self {
            config,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            compressed_proofs: Arc::new(Mutex::new(HashMap::new())),
            evict: Arc::new(Mutex::new(HashMap::new())),
            semaphore,
        });

        // Background TTL eviction — sweeps every 60s.
        // CompressedData entries are evicted alongside jobs. The aggregator
        // must consume compressed_proofs before TTL, or the batch will be
        // incomplete. TTL should be set well above the aggregator window.
        {
            let jobs              = Arc::clone(&mgr.jobs);
            let compressed_proofs = Arc::clone(&mgr.compressed_proofs);
            let evict             = Arc::clone(&mgr.evict);
            let ttl               = mgr.config.job_ttl_secs;
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(60));
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
                        compressed_proofs.lock().await.remove(id);
                        info!(%id, "job evicted after TTL");
                    }
                }
            });
        }

        mgr
    }

    /// Submit a job. Returns the job_id immediately.
    ///
    /// Runs phase 1 (compressed) only. Compressed is the terminal success state.
    /// The attestation_hash is available as soon as the job reaches Compressed.
    ///
    /// The aggregator crate (not this manager) is responsible for collecting N
    /// CompressedData entries and running one Groth16 for on-chain settlement.
    pub async fn submit(&self, input_data: Vec<Vec<f64>>) -> Result<String> {
        let job_id = Uuid::new_v4().to_string();
        let config            = self.config.clone();
        let jobs              = Arc::clone(&self.jobs);
        let compressed_proofs = Arc::clone(&self.compressed_proofs);
        let evict             = Arc::clone(&self.evict);
        let sem               = Arc::clone(&self.semaphore);
        let timeout = std::time::Duration::from_secs(self.config.timeout_secs);

        jobs.lock().await.insert(job_id.clone(), JobState::Queued);
        info!(%job_id, "job queued");

        let job_id_bg = job_id.clone();
        tokio::spawn(async move {
            let result = tokio::time::timeout(timeout, async {
                let _permit = sem.acquire().await.unwrap();

                jobs.lock()
                    .await
                    .insert(job_id_bg.clone(), JobState::Running);
                info!(%job_id_bg, "job running — compressed proving starting");

                // ── Phase 1: Compressed ───────────────────────────────────────
                // This is the only proving phase per job. No Groth16 here.
                // The aggregator handles Groth16 for the batch.
                let phase1_result =
                    proving::prove_compressed(&config, &job_id_bg, input_data.clone()).await;

                let (compressed_data, raw_compressed_proof) = match phase1_result {
                    Ok(pair) => pair,
                    Err(e) => {
                        error!(%job_id_bg, "compressed proving failed: {e}");
                        jobs.lock().await.insert(
                            job_id_bg.clone(),
                            JobState::Failed {
                                reason: format!("compressed proof failed: {e}"),
                            },
                        );
                        evict
                            .lock()
                            .await
                            .insert(job_id_bg.clone(), std::time::Instant::now());
                        return;
                    }
                };

                let attestation_hash = compressed_data.attestation_hash;
                let vk = compressed_data.vk.clone().expect("vk must be set after phase 1");
                let public_values = raw_compressed_proof.public_values.to_vec();

                // Store CompressedData — the aggregator batch collector reads this.
                // Must be stored before setting JobState::Compressed so any
                // concurrent reader that sees Compressed can immediately fetch data.
                compressed_proofs.lock().await.insert(
                    job_id_bg.clone(),
                    CompressedData {
                        proof: raw_compressed_proof,
                        vk,
                        attestation_hash,
                        public_values,
                    },
                );

                // Compressed is the terminal success state.
                jobs.lock().await.insert(
                    job_id_bg.clone(),
                    JobState::Compressed { attestation_hash },
                );

                // Start TTL clock immediately — aggregator must consume
                // compressed_proofs before job_ttl_secs elapses.
                evict
                    .lock()
                    .await
                    .insert(job_id_bg.clone(), std::time::Instant::now());

                info!(
                    %job_id_bg,
                    attestation_hash = %hex::encode(attestation_hash),
                    "compressed proof ready — queued for aggregator batch"
                );
            })
            .await;

            if result.is_err() {
                error!(%job_id_bg, "job timed out during compressed proving");
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

    /// Returns the current state of a job.
    pub async fn status(&self, job_id: &str) -> Result<JobState> {
        self.jobs
            .lock()
            .await
            .get(job_id)
            .cloned()
            .ok_or_else(|| anyhow!("job not found: {job_id}"))
    }

    /// Returns CompressedData for a job — available after phase 1 completes.
    ///
    /// The aggregator uses this to build AggregationInput without re-proving.
    /// Returns an error if phase 1 is not yet complete or the job was evicted.
    pub async fn compressed_proof_data(&self, job_id: &str) -> Result<CompressedData> {
        self.compressed_proofs
            .lock()
            .await
            .get(job_id)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "compressed proof data not found for job '{job_id}' \
                     — phase 1 may not be complete yet, or job was evicted"
                )
            })
    }

    /// Returns the verifying key for a job — available after phase 1 completes.
    pub async fn verifying_key(&self, job_id: &str) -> Result<sp1_sdk::SP1VerifyingKey> {
        let cd = self.compressed_proof_data(job_id).await?;
        Ok(cd.vk)
    }

    /// Returns the attestation_hash for a job.
    ///
    /// Available as soon as the job reaches JobState::Compressed.
    /// Returns an error if the job is still proving or has failed.
    pub async fn attestation_hash(&self, job_id: &str) -> Result<[u8; 32]> {
        match self.status(job_id).await? {
            JobState::Compressed { attestation_hash } => Ok(attestation_hash),
            JobState::Queued | JobState::Running => Err(anyhow!(
                "attestation_hash not yet available — job is still proving"
            )),
            JobState::Failed { reason } => Err(anyhow!(
                "job failed before attestation_hash was computed: {reason}"
            )),
        }
    }
}