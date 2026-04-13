use alloy::{
    network::EthereumWallet,
    primitives::{Address, U256},
    providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
    sol,
};
use std::str::FromStr;
use std::time::Duration;
use tracing::{info, warn};

use crate::{config::SettlerConfig, error::SettlerError};

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    VeilVault,
    r#"[
        {
            "type": "function",
            "name": "balanceOf",
            "inputs":  [{ "name": "user", "type": "address" }],
            "outputs": [{ "name": "",     "type": "uint256" }],
            "stateMutability": "view"
        },
        {
            "type": "function",
            "name": "canProve",
            "inputs": [
                { "name": "user", "type": "address" },
                { "name": "tier", "type": "uint8"   }
            ],
            "outputs": [{ "name": "", "type": "bool" }],
            "stateMutability": "view"
        },
        {
            "type": "function",
            "name": "deductFee",
            "inputs": [
                { "name": "user",  "type": "address" },
                { "name": "tier",  "type": "uint8"   },
                { "name": "jobId", "type": "string"  }
            ],
            "outputs": [],
            "stateMutability": "nonpayable"
        },
        {
            "type": "function",
            "name": "STANDARD_FEE",
            "inputs":  [],
            "outputs": [{ "name": "", "type": "uint256" }],
            "stateMutability": "view"
        },
        {
            "type": "function",
            "name": "PRIORITY_FEE",
            "inputs":  [],
            "outputs": [{ "name": "", "type": "uint256" }],
            "stateMutability": "view"
        }
    ]"#
);

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum ProofTier {
    Standard = 0,
    Priority = 1,
}

#[derive(Debug, Clone)]
pub struct VaultClient {
    config: SettlerConfig,
    vault_address: String,
    tx_timeout_secs: u64,
}

impl VaultClient {
    pub fn new(config: SettlerConfig, vault_address: String) -> Self {
        Self {
            tx_timeout_secs: config.tx_timeout_secs,
            config,
            vault_address,
        }
    }

    async fn build_provider(&self) -> Result<impl alloy::providers::Provider, SettlerError> {
        let key = self.config.private_key.trim_start_matches("0x");
        let signer =
            PrivateKeySigner::from_str(key).map_err(|e| SettlerError::InvalidKey(e.to_string()))?;
        let wallet = EthereumWallet::from(signer);
        ProviderBuilder::new()
            .wallet(wallet)
            .connect(&self.config.rpc_url)
            .await
            .map_err(|e| SettlerError::RpcError(e.to_string()))
    }

    fn vault_addr(&self) -> Result<Address, SettlerError> {
        Address::from_str(&self.vault_address)
            .map_err(|e| SettlerError::InvalidAddress(e.to_string()))
    }

    fn parse_addr(addr: &str) -> Result<Address, SettlerError> {
        Address::from_str(addr).map_err(|e| SettlerError::InvalidAddress(e.to_string()))
    }

    /// Get raw HSK balance for `user` in wei.
    ///
    /// NOTE: Alloy's .call() return type depends on version.
    /// In the version used here, single-output functions return the value
    /// directly (U256), not a wrapper struct. Use the return value as-is.
    pub async fn balance_of(&self, user: &str) -> Result<U256, SettlerError> {
        let provider = self.build_provider().await?;
        let contract = VeilVault::new(self.vault_addr()?, &provider);
        let user_addr = Self::parse_addr(user)?;

        contract.balanceOf(user_addr).call().await.map_err(|e| {
            warn!(user, "balanceOf call failed: {e}");
            SettlerError::RpcError(e.to_string())
        })
    }

    /// Check whether `user` has enough balance for `tier`.
    pub async fn check_balance(&self, user: &str, tier: ProofTier) -> Result<bool, SettlerError> {
        let provider = self.build_provider().await?;
        let contract = VeilVault::new(self.vault_addr()?, &provider);
        let user_addr = Self::parse_addr(user)?;

        contract
            .canProve(user_addr, tier as u8)
            .call()
            .await
            .map_err(|e| {
                warn!(user, "canProve call failed: {e}");
                SettlerError::RpcError(e.to_string())
            })
    }

    /// Deduct the proof fee from `user`'s vault balance.
    /// Call AFTER the proof is submitted to Succinct — not before.
    pub async fn deduct_fee(
        &self,
        user: &str,
        tier: ProofTier,
        job_id: &str,
    ) -> Result<String, SettlerError> {
        let provider = self.build_provider().await?;
        let contract = VeilVault::new(self.vault_addr()?, &provider);
        let user_addr = Self::parse_addr(user)?;

        info!(user, job_id, tier = ?tier, "deducting proof fee from VeilVault");

        let pending = contract
            .deductFee(user_addr, tier as u8, job_id.to_string())
            .send()
            .await
            .map_err(|e| SettlerError::RpcError(e.to_string()))?;

        let tx_hash = *pending.tx_hash();

        let receipt = tokio::time::timeout(
            Duration::from_secs(self.tx_timeout_secs),
            pending.get_receipt(),
        )
        .await
        .map_err(|_| SettlerError::TxTimeout(self.tx_timeout_secs))?
        .map_err(|e| SettlerError::RpcError(e.to_string()))?;

        if !receipt.status() {
            return Err(SettlerError::TxReverted(format!("{tx_hash:#x}")));
        }

        let hash_str = format!("{tx_hash:#x}");
        info!(user, job_id, tx_hash = %hash_str, "fee deducted successfully");
        Ok(hash_str)
    }
}
