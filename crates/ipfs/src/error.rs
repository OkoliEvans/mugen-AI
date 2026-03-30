use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpfsError {
    #[error("Pinata request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Pinata API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("Missing env var: {0}")]
    Config(String),
}