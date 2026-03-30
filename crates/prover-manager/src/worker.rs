use crate::error::ProverError;
use crate::job::{ProveJob, WorkerResult};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, error, instrument};

/// Spawns `worker.py` as a subprocess, feeds the job via stdin,
/// reads the JSON result from stdout, and returns it.
///
/// The Python process is killed automatically if it exceeds `timeout_secs`.
#[instrument(skip(job), fields(job_id = %job.job_id))]
pub async fn run_worker(
    job: &ProveJob,
    python_bin: &str,
    worker_script: &str,
    timeout_secs: u64,
) -> Result<WorkerResult, ProverError> {
    let payload = serde_json::to_string(job)?;

    debug!("spawning worker subprocess");

    let mut child = Command::new(python_bin)
        .arg(worker_script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true) // ensures cleanup if we timeout or drop
        .spawn()?;

    // Write job payload to stdin then close it
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).await?;
        // stdin closed here — worker sees EOF and starts processing
    }

    // Wait for the process with a hard timeout
    let output = timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
        .await
        .map_err(|_| ProverError::Timeout(timeout_secs))?
        .map_err(ProverError::SpawnFailed)?;

    if !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!(stderr = %stderr, "worker stderr");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.trim();

    if line.is_empty() {
        error!("worker produced no output");
        return Err(ProverError::NoOutput);
    }

    let result: WorkerResult = serde_json::from_str(line)?;

    if let crate::job::WorkerStatus::Error = result.status {
        let reason = result.error.clone().unwrap_or_else(|| "unknown".into());
        error!(reason = %reason, "worker reported failure");
        return Err(ProverError::WorkerError(reason));
    }

    Ok(result)
}