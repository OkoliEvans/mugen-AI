//! Pinata v2 API client.
//!
//! Supports two pinning operations:
//!   - `pin_bytes`  — arbitrary bytes with a filename (model artifacts, proof files)
//!   - `pin_json`   — serialisable value pinned as JSON (proof metadata)
//!
//! Both return the IPFS CID string on success.

use bytes::Bytes;
use reqwest::{
    multipart::{Form, Part},
    Client, StatusCode,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::error::IpfsError;

const PINATA_API_BASE: &str = "https://api.pinata.cloud";

// ── Response shapes ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct PinResponse {
    #[serde(rename = "IpfsHash")]
    ipfs_hash: String,
    #[serde(rename = "PinSize")]
    #[allow(dead_code)]
    pin_size: u64,
}

#[derive(Debug, Deserialize)]
struct PinataErrorBody {
    error: Option<PinataErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct PinataErrorDetail {
    details: Option<String>,
    reason: Option<String>,
}

// ── Pin metadata ──────────────────────────────────────────────────────────────

/// Optional metadata attached to every pin — visible in the Pinata dashboard.
#[derive(Debug, Serialize)]
pub struct PinMeta {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyvalues: Option<serde_json::Value>,
}

// ── Client ────────────────────────────────────────────────────────────────────

/// Pinata IPFS client. Cheap to clone — wraps an Arc internally via reqwest.
#[derive(Clone)]
pub struct PinataClient {
    http:        Client,
    jwt:         String,
    gateway_url: String,
}

impl PinataClient {
    /// Construct from explicit values. Prefer [`PinataClient::from_env`] in production.
    pub fn new(jwt: impl Into<String>, gateway_url: impl Into<String>) -> Self {
        Self {
            http:        Client::new(),
            jwt:         jwt.into(),
            gateway_url: gateway_url.into(),
        }
    }

    /// Construct from environment variables.
    ///
    /// Required:
    ///   `PINATA_JWT`          — Pinata v2 JWT (Admin or pinFileToIPFS scope)
    ///   `PINATA_GATEWAY_URL`  — Your dedicated Pinata gateway hostname
    ///                           e.g. `green-gigantic-muskox-661.mypinata.cloud`
    pub fn from_env() -> Result<Self, IpfsError> {
        let jwt = std::env::var("PINATA_JWT")
            .map_err(|_| IpfsError::Config("PINATA_JWT not set".into()))?;
        let gateway_url = std::env::var("PINATA_GATEWAY_URL")
            .map_err(|_| IpfsError::Config("PINATA_GATEWAY_URL not set".into()))?;

        Ok(Self::new(jwt, gateway_url))
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Pin raw bytes to IPFS via Pinata.
    ///
    /// `filename` is used as the display name in the Pinata dashboard and as
    /// the multipart filename — use something descriptive like `tiny_mlp_v1.onnx`
    /// or `proof_<job_id>.json`.
    ///
    /// Returns the IPFS CID string (e.g. `QmXyz...` or `bafy...`).
    pub async fn pin_bytes(
        &self,
        data:     Bytes,
        filename: impl Into<String>,
        meta:     Option<PinMeta>,
    ) -> Result<String, IpfsError> {
        let filename = filename.into();
        let size     = data.len();

        debug!(filename, size, "pinning bytes to IPFS");

        let file_part = Part::bytes(data.to_vec())
            .file_name(filename.clone())
            .mime_str("application/octet-stream")
            .expect("valid mime");

        let mut form = Form::new().part("file", file_part);

        if let Some(m) = meta {
            let meta_json = serde_json::to_string(&m).unwrap_or_default();
            form = form.text("pinataMetadata", meta_json);
        }

        let cid = self
            .post_multipart("/pinning/pinFileToIPFS", form)
            .await?;

        info!(filename, %cid, size, "pinned bytes to IPFS");
        Ok(cid)
    }

    /// Pin a JSON-serialisable value to IPFS via Pinata.
    ///
    /// Returns the IPFS CID string.
    pub async fn pin_json<T: Serialize>(
        &self,
        value: &T,
        meta:  Option<PinMeta>,
    ) -> Result<String, IpfsError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct PinJsonRequest<'a, T: Serialize> {
            #[serde(rename = "pinataContent")]
            pinata_content:  &'a T,
            #[serde(rename = "pinataMetadata", skip_serializing_if = "Option::is_none")]
            pinata_metadata: Option<&'a PinMeta>,
        }

        let body = PinJsonRequest {
            pinata_content:  value,
            pinata_metadata: meta.as_ref(),
        };

        debug!("pinning JSON to IPFS");

        let resp = self
            .http
            .post(format!("{PINATA_API_BASE}/pinning/pinJSONToIPFS"))
            .bearer_auth(&self.jwt)
            .json(&body)
            .send()
            .await?;

        let cid = self.handle_response(resp).await?;
        info!(%cid, "pinned JSON to IPFS");
        Ok(cid)
    }

    /// Build the public gateway URL for a CID.
    ///
    /// Returns: `https://<gateway_url>/ipfs/<cid>`
    pub fn gateway_url(&self, cid: &str) -> String {
        let host = self.gateway_url.trim_end_matches('/');
        // Ensure the host has a scheme
        if host.starts_with("http") {
            format!("{host}/ipfs/{cid}")
        } else {
            format!("https://{host}/ipfs/{cid}")
        }
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    async fn post_multipart(&self, path: &str, form: Form) -> Result<String, IpfsError> {
        let resp = self
            .http
            .post(format!("{PINATA_API_BASE}{path}"))
            .bearer_auth(&self.jwt)
            .multipart(form)
            .send()
            .await?;

        self.handle_response(resp).await
    }

    async fn handle_response(&self, resp: reqwest::Response) -> Result<String, IpfsError> {
        let status = resp.status();

        if status == StatusCode::OK || status == StatusCode::CREATED {
            let pin: PinResponse = resp.json().await?;
            return Ok(pin.ipfs_hash);
        }

        // Parse Pinata error body if possible
        let status_u16 = status.as_u16();
        let message = match resp.json::<PinataErrorBody>().await {
            Ok(body) => body
                .error
                .and_then(|e| e.details.or(e.reason))
                .unwrap_or_else(|| "unknown Pinata error".into()),
            Err(_) => format!("HTTP {status_u16}"),
        };

        Err(IpfsError::Api { status: status_u16, message })
    }
}