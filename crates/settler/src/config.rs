// crates/settler/src/config.rs

use crate::error::SettlerError;

/// Configuration for the Settler.
/// settlement is now direct to HashKey testnet.
#[derive(Debug, Clone)]
pub struct SettlerConfig {
    /// RPC endpoint — HashKey testnet (https://testnet.hsk.xyz)
    pub rpc_url: String,
    /// Hex-encoded private key of the settler wallet (SETTLER_PRIVATE_KEY)
    pub private_key: String,
    /// InferenceVerifier.sol address on HashKey testnet (INFERENCE_VERIFIER_ADDRESS)
    pub contract_address: String,
    /// Confirmations before tx is considered final (default: 1)
    pub confirmations: u64,
    /// Seconds before a pending tx times out (default: 120)
    pub tx_timeout_secs: u64,

    // ── Kept for gateway AppState compatibility — unused in HashKey path ──────
    pub eth_sepolia_inference_bridge: String,
    pub rpc: String,
    pub inference_verifier: String,
    pub poll_interval_secs: u64,
    pub max_poll_attempts: u32,
    pub bridge_fee_wei: u64,
}

impl SettlerConfig {
    pub fn from_env() -> Result<Self, SettlerError> {
        Ok(Self {
            rpc_url:          require_env("SETTLER_RPC_URL")?,
            private_key:      require_env("SETTLER_PRIVATE_KEY")?,
            contract_address: require_env("INFERENCE_VERIFIER_ADDRESS")?,
            confirmations:    optional_env_u64("SETTLER_CONFIRMATIONS", 1),
            tx_timeout_secs:  optional_env_u64("SETTLER_TX_TIMEOUT_SECS", 120),

            // Kept for struct compat — not used in HashKey settlement path
            eth_sepolia_inference_bridge: std::env::var("ETH_SEPOLIA_INFERENCE_BRIDGE")
                .unwrap_or_default(),
            rpc: std::env::var("RPC").unwrap_or_default(),
            inference_verifier: std::env::var("INFERENCE_VERIFIER")
                .unwrap_or_default(),
            poll_interval_secs: optional_env_u64("POLL_INTERVAL_SECS", 15),
            max_poll_attempts:  optional_env_u64("MAX_POLL_ATTEMPTS", 24) as u32,
            bridge_fee_wei:     optional_env_u64("SETTLER_BRIDGE_FEE_WEI", 0),
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