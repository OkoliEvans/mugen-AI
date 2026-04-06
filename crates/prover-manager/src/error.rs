use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProverError {
    #[error("worker process failed to spawn: {0}")]
    SpawnFailed(#[from] std::io::Error),

    #[error("worker timed out after {0}s")]
    Timeout(u64),

    #[error("worker returned error: {0}")]
    WorkerError(String),

    #[error("failed to serialize job: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("worker produced no output")]
    NoOutput,

    #[error("proof file not found at {0}")]
    ProofNotFound(String),

    #[error("job queue is full")]
    QueueFull,

    #[error("job {0} not found")]
    JobNotFound(String),
}
