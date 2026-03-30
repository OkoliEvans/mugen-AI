use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Sent to the Python worker via stdin
#[derive(Debug, Clone, Serialize)]
pub struct ProveJob {
    pub job_id: String,
    pub input_data: Vec<Vec<f64>>,
    pub artifacts_dir: String,
}

/// Received from the Python worker via stdout
#[derive(Debug, Clone, Deserialize)]
pub struct WorkerResult {
    pub job_id: String,
    pub status: WorkerStatus,
    pub proof_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WorkerStatus {
    Ok,
    Error,
}

/// Internal job state tracked by the manager
#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Queued,
    Running,
    Done { proof_path: String },
    Failed { reason: String },
}

/// Full job record stored in the manager
#[derive(Debug, Clone)]
pub struct JobRecord {
    pub job_id: String,
    pub state: JobState,
    pub created_at: Instant,
}

impl JobRecord {
    pub fn new(job_id: String) -> Self {
        Self {
            job_id,
            state: JobState::Queued,
            created_at: Instant::now(),
        }
    }
}