//! # veil-sdk
//!
//! Rust client for the Mugen Veil verifiable inference gateway.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use veil_sdk::VeilClient;
//!
//! #[tokio::main]
//! async fn main() -> veil_sdk::error::Result<()> {
//!     let client = VeilClient::builder()
//!         .base_url("http://localhost:8080")
//!         .build()?;
//!
//!     // High-level: submit + wait
//!     let result = client
//!         .verify_inference("tiny_mlp_v1", vec![vec![0.1, 0.2, 0.3, 0.4]])
//!         .await?;
//!
//!     println!("status:          {}", result.status);
//!     println!("tx_hash:         {:?}", result.tx_hash);
//!     println!("attestation:     {:?}", result.attestation_hash);
//!     println!("elapsed:         {}ms", result.elapsed_ms);
//!     Ok(())
//! }
//! ```
//!
//! ## Primitives
//!
//! For finer control, use the primitive methods directly:
//!
//! ```rust,no_run
//! use veil_sdk::VeilClient;
//!
//! #[tokio::main]
//! async fn main() -> veil_sdk::error::Result<()> {
//!     let client = VeilClient::builder()
//!         .base_url("http://localhost:8080")
//!         .build()?;
//!
//!     // Health check
//!     let health = client.health_check().await?;
//!     assert!(health.is_healthy());
//!
//!     // Submit and poll manually
//!     let job_id = client
//!         .submit_job("tiny_mlp_v1", vec![vec![0.5, 0.3, 0.8, 0.1]])
//!         .await?;
//!
//!     loop {
//!         let job = client.get_job(&job_id).await?;
//!         if job.status.is_terminal() { break; }
//!         tokio::time::sleep(std::time::Duration::from_secs(2)).await;
//!     }
//!
//!     // Fetch raw proof bytes
//!     let proof = client.get_proof(&job_id).await?;
//!     println!("proof size: {} bytes", proof.size_bytes);
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod error;
pub mod types;

pub use client::{VeilClient, VeilClientBuilder};
pub use error::{Result, VeilError};
pub use types::{
    Health, Job, JobStatus, Proof, RegisterModelRequest, RegisterModelResponse, VerifyResult,
};
