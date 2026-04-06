use std::time::{Duration, Instant};

use reqwest::{header, StatusCode};
use serde::de::DeserializeOwned;
use tracing::{debug, info, warn};

use crate::{
    error::{Result, VeilError},
    types::{
        Health, Job, JobStatus, Proof, RegisterModelRequest, RegisterModelResponse,
        SubmitJobRequest, SubmitJobResponse, VerifyResult,
    },
};

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for [`VeilClient`].
///
/// ```rust
/// use veil_sdk::VeilClient;
/// use std::time::Duration;
///
/// let client = VeilClient::builder()
///     .base_url("http://localhost:8080")
///     .timeout(Duration::from_secs(600))
///     .poll_interval(Duration::from_secs(3))
///     .build()
///     .unwrap();
/// ```
#[derive(Debug)]
pub struct VeilClientBuilder {
    base_url: String,
    timeout: Duration,
    poll_interval: Duration,
}

impl Default for VeilClientBuilder {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8080".to_string(),
            timeout: Duration::from_secs(600),
            poll_interval: Duration::from_secs(3),
        }
    }
}

impl VeilClientBuilder {
    /// Base URL of the Veil gateway, e.g. `"https://api.mugen.network"`.
    /// Trailing slashes are stripped automatically.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Maximum wall-clock time to wait for a job to reach a terminal state.
    /// Applies to `verify_inference`. Defaults to 600 seconds.
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// How often to poll `GET /v1/jobs/{id}` while waiting.
    /// Defaults to 3 seconds.
    pub fn poll_interval(mut self, d: Duration) -> Self {
        self.poll_interval = d;
        self
    }

    /// Consume the builder and construct a [`VeilClient`].
    ///
    /// # Errors
    /// Returns [`VeilError::InvalidUrl`] if the base URL cannot be parsed by
    /// `reqwest`.
    pub fn build(self) -> Result<VeilClient> {
        // Validate the URL by attempting to construct a reqwest client
        // with a test request (parse-only, no network).
        reqwest::Url::parse(&self.base_url)
            .map_err(|e| VeilError::InvalidUrl(format!("{}: {e}", self.base_url)))?;

        let http = reqwest::Client::builder()
            .default_headers({
                let mut h = header::HeaderMap::new();
                h.insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("application/json"),
                );
                h.insert(
                    header::ACCEPT,
                    header::HeaderValue::from_static("application/json"),
                );
                h
            })
            // reqwest's own connection timeout — separate from our poll timeout.
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(VeilError::Http)?;

        Ok(VeilClient {
            http,
            base_url: self.base_url,
            timeout: self.timeout,
            poll_interval: self.poll_interval,
        })
    }
}

// ── Client ────────────────────────────────────────────────────────────────────

/// Async client for the Mugen Veil verifiable inference gateway.
///
/// Construct via [`VeilClient::builder()`].
///
/// `VeilClient` is cheap to clone — the underlying `reqwest::Client` uses an
/// `Arc` internally and shares the connection pool across clones.
#[derive(Debug, Clone)]
pub struct VeilClient {
    http: reqwest::Client,
    base_url: String,
    timeout: Duration,
    poll_interval: Duration,
}

impl VeilClient {
    /// Begin building a client. See [`VeilClientBuilder`].
    pub fn builder() -> VeilClientBuilder {
        VeilClientBuilder::default()
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// Parse a response, surfacing gateway error bodies as [`VeilError::Api`].
    async fn parse<T: DeserializeOwned>(&self, res: reqwest::Response) -> Result<T> {
        let status = res.status();
        if status.is_success() {
            Ok(res.json::<T>().await?)
        } else {
            // Best-effort extraction of an `error` field from the JSON body.
            let message = res
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                .unwrap_or_else(|| status.to_string());

            Err(VeilError::Api {
                status: status.as_u16(),
                message,
            })
        }
    }

    // ── Primitive methods ─────────────────────────────────────────────────────

    /// `GET /healthz` — Returns gateway health information.
    ///
    /// Use [`Health::is_healthy()`] to check overall readiness.
    pub async fn health_check(&self) -> Result<Health> {
        debug!("GET /healthz");
        let res = self.http.get(self.url("/healthz")).send().await?;
        self.parse(res).await
    }

    /// `POST /v1/jobs` — Submit an inference job and return immediately.
    ///
    /// Returns the `job_id`. Use [`get_job`](Self::get_job) to poll status,
    /// or [`verify_inference`](Self::verify_inference) to submit and wait.
    ///
    /// # Arguments
    /// - `model_id`   — the model name registered with the gateway (e.g. `"tiny_mlp_v1"`)
    /// - `input_data` — row-major input tensor, e.g. `vec![vec![0.1, 0.2, 0.3, 0.4]]`
    pub async fn submit_job(
        &self,
        model_id: impl Into<String>,
        input_data: Vec<Vec<f64>>,
    ) -> Result<String> {
        let body = SubmitJobRequest {
            input_data,
            model_id: model_id.into(),
        };

        debug!(model_id = %body.model_id, "POST /v1/jobs");

        let res = self
            .http
            .post(self.url("/v1/jobs"))
            .json(&body)
            .send()
            .await?;

        let resp: SubmitJobResponse = self.parse(res).await?;
        info!(job_id = %resp.job_id, "job submitted");
        Ok(resp.job_id)
    }

    /// `GET /v1/jobs/{id}` — Poll the status of a job.
    pub async fn get_job(&self, job_id: &str) -> Result<Job> {
        debug!(%job_id, "GET /v1/jobs/{job_id}");
        let res = self
            .http
            .get(self.url(&format!("/v1/jobs/{job_id}")))
            .send()
            .await?;
        self.parse(res).await
    }

    /// `GET /v1/jobs/{id}/proof` — Fetch the raw proof bytes for a completed job.
    ///
    /// The gateway returns `HTTP 202` if the job is not yet complete.
    /// Returns [`VeilError::Api`] with status 202 in that case — callers should
    /// poll [`get_job`](Self::get_job) first.
    pub async fn get_proof(&self, job_id: &str) -> Result<Proof> {
        debug!(%job_id, "GET /v1/jobs/{job_id}/proof");
        let res = self
            .http
            .get(self.url(&format!("/v1/jobs/{job_id}/proof")))
            .send()
            .await?;
        self.parse(res).await
    }

    /// `POST /v1/models` — Register an ONNX model artifact with the gateway.
    ///
    /// Pins the artifact to IPFS and registers it on-chain.
    /// Requires `PINATA_JWT` to be configured on the gateway.
    pub async fn register_model(&self, req: RegisterModelRequest) -> Result<RegisterModelResponse> {
        debug!(name = %req.name, version = %req.version, "POST /v1/models");
        let res = self
            .http
            .post(self.url("/v1/models"))
            .json(&req)
            .send()
            .await?;
        self.parse(res).await
    }

    // ── High-level method ─────────────────────────────────────────────────────

    /// Submit an inference job and block until it reaches a terminal state.
    ///
    /// Polls `GET /v1/jobs/{id}` at the configured `poll_interval` until the
    /// job status is one of `done`, `settled`, or `failed`, or until the
    /// configured `timeout` elapses.
    ///
    /// # Arguments
    /// - `model_id`   — registered model name, e.g. `"tiny_mlp_v1"`
    /// - `input_data` — row-major input tensor
    ///
    /// # Errors
    /// - [`VeilError::Timeout`]    — polling exceeded the configured timeout
    /// - [`VeilError::JobFailed`]  — the gateway reported the job as failed
    /// - [`VeilError::Api`]        — gateway returned a non-2xx response
    /// - [`VeilError::Http`]       — network-level failure
    ///
    /// # Example
    /// ```rust,no_run
    /// # use veil_sdk::VeilClient;
    /// # #[tokio::main] async fn main() -> veil_sdk::error::Result<()> {
    /// let client = VeilClient::builder()
    ///     .base_url("http://localhost:8080")
    ///     .build()?;
    ///
    /// let result = client
    ///     .verify_inference("tiny_mlp_v1", vec![vec![0.1, 0.2, 0.3, 0.4]])
    ///     .await?;
    ///
    /// println!("tx_hash: {:?}", result.tx_hash);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn verify_inference(
        &self,
        model_id: impl Into<String>,
        input_data: Vec<Vec<f64>>,
    ) -> Result<VerifyResult> {
        let model_id = model_id.into();
        let started = Instant::now();

        // 1. Submit
        let job_id = self.submit_job(&model_id, input_data).await?;
        info!(%job_id, %model_id, "job submitted — polling until terminal state");

        // 2. Poll
        let deadline = started + self.timeout;
        let mut last_status = String::from("queued");

        loop {
            tokio::time::sleep(self.poll_interval).await;

            if Instant::now() >= deadline {
                return Err(VeilError::Timeout {
                    job_id,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    last_status,
                });
            }

            let job = match self.get_job(&job_id).await {
                Ok(j) => j,
                Err(e) => {
                    // Transient network errors during polling are logged and
                    // retried rather than propagated immediately.
                    warn!(%job_id, "poll error (will retry): {e}");
                    continue;
                }
            };

            last_status = job.status.to_string();
            debug!(%job_id, status = %last_status, "poll");

            match &job.status {
                JobStatus::Failed => {
                    return Err(VeilError::JobFailed {
                        job_id,
                        reason: job.reason,
                    });
                }
                s if s.is_terminal() => {
                    let elapsed_ms = started.elapsed().as_millis() as u64;
                    info!(%job_id, status = %last_status, elapsed_ms, "job complete");
                    return Ok(VerifyResult {
                        job_id,
                        status: job.status,
                        tx_hash: job.tx_hash,
                        attestation_hash: job.attestation_hash,
                        elapsed_ms,
                    });
                }
                _ => {} // still in progress — keep polling
            }
        }
    }
}
