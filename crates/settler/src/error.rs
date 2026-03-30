use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettlerError {
    #[error("environment variable missing: {0}")]
    MissingEnv(String),

    #[error("invalid private key: {0}")]
    InvalidKey(String),

    #[error("invalid contract address: {0}")]
    InvalidAddress(String),

    #[error("proof file not found at {0}")]
    ProofNotFound(String),

    #[error("failed to read proof file: {0}")]
    ProofReadError(String),

    #[error("invalid proof JSON: {0}")]
    ProofParseError(String),

    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("transaction reverted: {0}")]
    TxReverted(String),

    #[error("transaction timed out after {0}s")]
    TxTimeout(u64),
}