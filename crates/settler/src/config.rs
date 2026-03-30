use crate::error::SettlerError;

/// Configuration loaded from environment / .env file.
#[derive(Debug, Clone)]
pub struct SettlerConfig {
    /// Ethereum RPC endpoint (e.g. https://ethereum-sepolia-rpc.publicnode.com)
    pub rpc_url: String,

    /// Hex-encoded private key of the whitelisted settler wallet (with or without 0x prefix)
    pub private_key: String,

    /// Deployed InferenceVerifier contract address
    pub contract_address: String,

    /// Confirmations to wait for before considering a tx final (default: 1)
    pub confirmations: u64,

    /// Seconds before a pending tx is considered timed out (default: 120)
    pub tx_timeout_secs: u64,
}

impl SettlerConfig {
    /// Load config from environment variables.
    /// Call `dotenvy::dotenv().ok()` before this in main.
    pub fn from_env() -> Result<Self, SettlerError> {
        Ok(Self {
            rpc_url: require_env("SETTLER_RPC_URL")?,
            private_key: require_env("SETTLER_PRIVATE_KEY")?,
            contract_address: require_env("INFERENCE_VERIFIER_ADDRESS")?,
            confirmations: optional_env_u64("SETTLER_CONFIRMATIONS", 1),
            tx_timeout_secs: optional_env_u64("SETTLER_TX_TIMEOUT_SECS", 120),
        })
    }
}

fn require_env(key: &str) -> Result<String, SettlerError> {
    std::env::var(key).map_err(|_| SettlerError::MissingEnv(key.to_string()))
}

fn optional_env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
