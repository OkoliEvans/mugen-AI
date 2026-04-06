use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettlerError {
    // ── Existing variants (unchanged) ─────────────────────────────────────────
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

    // ── New variants (additive) ───────────────────────────────────────────────
    /// Missing or invalid config for a specific chain path (e.g. starknet env vars not set)
    #[error("config error: {0}")]
    ConfigError(String),

    /// StarkNet L1→L2 settlement did not confirm within the polling window.
    #[error("starknet settlement not confirmed after {attempts} attempts ({interval_secs}s each)")]
    StarknetTimeout { attempts: u32, interval_secs: u64 },
}
