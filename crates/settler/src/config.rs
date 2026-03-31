use crate::error::SettlerError;

/// Configuration for the Settler. Loaded once at startup via `from_env()`.
#[derive(Debug, Clone)]
pub struct SettlerConfig {
    // ── Eth Sepolia — primary EVM chain ──────────────────────────────────────
    /// RPC endpoint for Eth Sepolia (SETTLER_RPC_URL — name preserved)
    pub rpc_url:          String,
    /// Hex-encoded private key of the settler wallet (SETTLER_PRIVATE_KEY)
    pub private_key:      String,
    /// InferenceVerifier.sol address — used only by register_model()
    pub contract_address: String,
    /// Confirmations before tx is considered final (default: 1)
    pub confirmations:    u64,
    /// Seconds before a pending tx times out (default: 120)
    pub tx_timeout_secs:  u64,

    // ── StarkNet settlement ───────────────────────────────────────────────────
    /// InferenceBridge.sol address on Eth Sepolia — KZG check + L1→L2 relay
    pub eth_sepolia_inference_bridge: String,
    /// StarkNet Sepolia RPC endpoint
    pub starknet_rpc:                 String,
    /// InferenceVerifier.cairo felt252 address on StarkNet Sepolia
    pub starknet_inference_verifier:  String,
    /// Poll interval while waiting for L1→L2 settlement (default: 15s)
    pub starknet_poll_interval_secs:  u64,
    /// Max poll attempts before giving up (default: 24 → 6 min total)
    pub starknet_max_poll_attempts:   u32,
    /// Wei forwarded as msg.value to cover StarkNet L1→L2 messaging fee
    /// Default: 30_000_000_000_000_000 (0.03 ETH) — sufficient for Sepolia
    pub starknet_bridge_fee_wei:      u64,
}

impl SettlerConfig {
    /// Load config from environment variables.
    /// Call `dotenvy::dotenv().ok()` before this in main.
    pub fn from_env() -> Result<Self, SettlerError> {
        Ok(Self {
            // Existing env var names preserved exactly
            rpc_url:          require_env("SETTLER_RPC_URL")?,
            private_key:      require_env("SETTLER_PRIVATE_KEY")?,
            contract_address: require_env("INFERENCE_VERIFIER_ADDRESS")?,
            confirmations:    optional_env_u64("SETTLER_CONFIRMATIONS", 1),
            tx_timeout_secs:  optional_env_u64("SETTLER_TX_TIMEOUT_SECS", 120),

            // StarkNet settlement — required for submit(), optional for register_model()
            eth_sepolia_inference_bridge: std::env::var("ETH_SEPOLIA_INFERENCE_BRIDGE")
                .unwrap_or_default(),
            starknet_rpc: std::env::var("STARKNET_RPC")
                .unwrap_or_else(|_| "https://starknet-sepolia.public.blastapi.io/rpc/v0_7".into()),
            starknet_inference_verifier: std::env::var("STARKNET_INFERENCE_VERIFIER")
                .unwrap_or_default(),
            starknet_poll_interval_secs: optional_env_u64("STARKNET_POLL_INTERVAL_SECS", 15),
            starknet_max_poll_attempts:  optional_env_u64("STARKNET_MAX_POLL_ATTEMPTS", 24) as u32,
            starknet_bridge_fee_wei:     optional_env_u64(
                "SETTLER_STARKNET_BRIDGE_FEE_WEI",
                30_000_000_000_000_000, // 0.03 ETH — covers Sepolia fees
            ),
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