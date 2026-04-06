use thiserror::Error;

/// All errors the SDK can return.
#[derive(Debug, Error)]
pub enum VeilError {
    /// Network or transport-level failure.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// The gateway returned a non-2xx status with an error body.
    #[error("gateway error {status}: {message}")]
    Api { status: u16, message: String },

    /// Polling timed out before the job reached a terminal state.
    #[error("job {job_id} timed out after {elapsed_ms}ms (last status: {last_status})")]
    Timeout {
        job_id: String,
        elapsed_ms: u64,
        last_status: String,
    },

    /// The gateway reported the job as failed.
    #[error("job {job_id} failed: {}", reason.as_deref().unwrap_or("no reason given"))]
    JobFailed {
        job_id: String,
        reason: Option<String>,
    },

    /// The gateway URL is not valid.
    #[error("invalid base URL: {0}")]
    InvalidUrl(String),

    /// Response body could not be parsed.
    #[error("failed to deserialise response: {0}")]
    Deserialize(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, VeilError>;
