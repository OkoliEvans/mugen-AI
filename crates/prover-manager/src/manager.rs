use crate::error::ProverError;
use crate::job::{JobRecord, JobState, ProveJob};
use crate::worker::run_worker;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tracing::{info, instrument};
use uuid::Uuid;

/// Configuration for the ProverManager
#[derive(Debug, Clone)]
pub struct ProverConfig {
    /// Path to the Python binary (e.g. "python3" or "/path/to/.venv/bin/python3")
    pub python_bin: String,
    /// Path to worker.py
    pub worker_script: String,
    /// Path to the artifacts directory (model.compiled, pk.key, vk.key)
    pub artifacts_dir: String,
    /// Max concurrent proof jobs (each job spawns one Python process)
    pub max_concurrent: usize,
    /// Per-job timeout in seconds
    pub timeout_secs: u64,
}

impl Default for ProverConfig {
    fn default() -> Self {
        Self {
            python_bin: "python3".into(),
            worker_script: "./prover/worker.py".into(),
            artifacts_dir: "./prover/artifacts".into(),
            max_concurrent: 4,
            timeout_secs: 60,
        }
    }
}

/// Thread-safe prover manager.
/// Clone freely — all clones share the same state.
#[derive(Clone)]
pub struct ProverManager {
    config: ProverConfig,
    semaphore: Arc<Semaphore>,
    jobs: Arc<Mutex<HashMap<String, JobRecord>>>,
}

impl ProverManager {
    pub fn new(config: ProverConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent));
        Self {
            config,
            semaphore,
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Submit a proof job. Returns the job_id immediately.
    /// The job runs in the background — poll `status()` to track it.
    #[instrument(skip(self, input_data))]
    pub async fn submit(&self, input_data: Vec<Vec<f64>>) -> Result<String, ProverError> {
        let job_id = Uuid::new_v4().to_string();

        // Register job as queued
        {
            let mut jobs = self.jobs.lock().await;
            jobs.insert(job_id.clone(), JobRecord::new(job_id.clone()));
        }

        info!(job_id = %job_id, "job queued");

        // Spawn background task — does not block the caller
        let manager = self.clone();
        let jid = job_id.clone();
        tokio::spawn(async move {
            manager.run_job(jid, input_data).await;
        });

        Ok(job_id)
    }

    /// Internal: acquires semaphore slot, runs the worker, updates job state
    async fn run_job(&self, job_id: String, input_data: Vec<Vec<f64>>) {
        // Acquire concurrency slot — blocks here if max_concurrent is reached
        let _permit = self.semaphore.acquire().await.expect("semaphore closed");

        // Mark as running
        {
            let mut jobs = self.jobs.lock().await;
            if let Some(record) = jobs.get_mut(&job_id) {
                record.state = JobState::Running;
            }
        }

        info!(job_id = %job_id, "job started");

        let job = ProveJob {
            job_id: job_id.clone(),
            input_data,
            artifacts_dir: self.config.artifacts_dir.clone(),
        };

        let result = run_worker(
            &job,
            &self.config.python_bin,
            &self.config.worker_script,
            self.config.timeout_secs,
        )
        .await;

        // Update job state based on result
        let mut jobs = self.jobs.lock().await;
        if let Some(record) = jobs.get_mut(&job_id) {
            match result {
                Ok(worker_result) => {
                    let proof_path = worker_result
                        .proof_path
                        .unwrap_or_else(|| format!("/tmp/{job_id}_proof.json"));
                    info!(job_id = %job_id, proof_path = %proof_path, "job done");
                    record.state = JobState::Done { proof_path };
                }
                Err(e) => {
                    info!(job_id = %job_id, error = %e, "job failed");
                    record.state = JobState::Failed {
                        reason: e.to_string(),
                    };
                }
            }
        }
        // _permit dropped here — releases concurrency slot
    }

    /// Get the current state of a job
    pub async fn status(&self, job_id: &str) -> Result<JobState, ProverError> {
        let jobs = self.jobs.lock().await;
        jobs.get(job_id)
            .map(|r| r.state.clone())
            .ok_or_else(|| ProverError::JobNotFound(job_id.into()))
    }

    /// Read the proof bytes for a completed job.
    /// Returns an error if the job is not done yet.
    pub async fn read_proof(&self, job_id: &str) -> Result<Vec<u8>, ProverError> {
        let state = self.status(job_id).await?;
        match state {
            JobState::Done { proof_path } => {
                tokio::fs::read(&proof_path)
                    .await
                    .map_err(|_| ProverError::ProofNotFound(proof_path))
            }
            _ => Err(ProverError::JobNotFound(job_id.into())),
        }
    }

    /// How many jobs are currently running
    pub async fn running_count(&self) -> usize {
        let jobs = self.jobs.lock().await;
        jobs.values()
            .filter(|r| r.state == JobState::Running)
            .count()
    }

    /// How many slots are available right now
    pub fn available_slots(&self) -> usize {
        self.semaphore.available_permits()
    }
}