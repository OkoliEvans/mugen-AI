//! `ipfs` — Pinata IPFS client for the Elenxis gateway.
//!
//! # Usage
//!
//! ```rust,no_run
//! use ipfs::{PinataClient, PinMeta};
//! use bytes::Bytes;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Reads PINATA_JWT and PINATA_GATEWAY_URL from env
//!     let client = PinataClient::from_env().unwrap();
//!
//!     // Pin a model artifact
//!     let artifact = std::fs::read("tiny_mlp_v1.onnx").unwrap();
//!     let cid = client
//!         .pin_bytes(
//!             Bytes::from(artifact),
//!             "tiny_mlp_v1.onnx",
//!             Some(PinMeta {
//!                 name: "tiny_mlp_v1".into(),
//!                 keyvalues: None,
//!             }),
//!         )
//!         .await
//!         .unwrap();
//!
//!     println!("CID: {cid}");
//!     println!("Gateway URL: {}", client.gateway_url(&cid));
//! }
//! ```

pub mod error;
pub mod pinata;

pub use error::IpfsError;
pub use pinata::{PinMeta, PinataClient};
