use alloy::{
    network::EthereumWallet,
    primitives::{Address, FixedBytes, U256, keccak256},
    providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
};
use std::str::FromStr;
use std::time::Duration;
use tracing::{info, warn};

use crate::{
    config::SettlerConfig,
    contract::{InferenceBridge, InferenceVerifier, ParsedProof},
    error::SettlerError,
};

/// Submits ZK proofs via the StarkNet settlement path:
///
///   1. Calls InferenceBridge.verifyAndBridge() on Eth Sepolia —
///      runs the KZG pairing check on L1 and relays the result to
///      InferenceVerifier.cairo via StarkNet L1→L2 messaging.
///   2. Polls InferenceVerifier.cairo::is_inference_verified() until confirmed.
///
/// Returns the Eth Sepolia tx hash once both steps complete.
pub struct Settler {
    config: SettlerConfig,
}

impl Settler {
    pub fn new(config: SettlerConfig) -> Self {
        Self { config }
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Submit a proof for a completed job.
    ///
    /// Calls InferenceBridge.verifyAndBridge() on Eth Sepolia, then blocks
    /// until InferenceVerifier.cairo confirms. Returns the EVM tx hash, or
    /// "already-verified" if already bridged.
    ///
    /// # Hash derivation
    ///   modelHash   = keccak256(model_name || model_version) → low 128 bits zeroed high
    ///   inferenceId = keccak256(input_data || output_data)   → low 128 bits zeroed high
    ///
    ///   Both are truncated to 128 bits so they safely fit within the StarkNet
    ///   felt252 field (max 2^251). Full 256-bit keccak values regularly exceed
    ///   felt252 range and cause OUT_OF_BOUND_PAYLOAD on sendMessageToL2().
    pub async fn submit(
        &self,
        proof_path:    &str,
        model_name:    &str,
        model_version: &str,
        input_data:    &[u8],
        output_data:   &[u8],
    ) -> Result<String, SettlerError> {
        if self.config.eth_sepolia_inference_bridge.is_empty() {
            return Err(SettlerError::ConfigError(
                "ETH_SEPOLIA_INFERENCE_BRIDGE not set".into(),
            ));
        }
        if self.config.starknet_inference_verifier.is_empty() {
            return Err(SettlerError::ConfigError(
                "STARKNET_INFERENCE_VERIFIER not set".into(),
            ));
        }

        info!(model_name, "submitting proof to InferenceBridge (Eth Sepolia) for StarkNet settlement");

        let inference_id = Self::derive_inference_id(input_data, output_data);

        // Step 1: KZG check + L1→L2 relay
        let evm_tx = self
            .bridge_submit(proof_path, model_name, model_version, &inference_id)
            .await?;

        info!(%evm_tx, %inference_id, "bridge tx confirmed — polling StarkNet for settlement");

        // Step 2: block until InferenceVerifier.cairo records the inference_id
        self.poll_starknet_settlement(&inference_id).await?;

        info!(%evm_tx, "StarkNet settlement confirmed");
        Ok(evm_tx)
    }

    /// Register a model on InferenceVerifier.sol (Eth Sepolia).
    ///
    /// Model registration is EVM-only — the StarkNet verifier reads model
    /// metadata through the bridge.
    ///
    /// # inputShapeHash derivation
    ///   keccak256(abi_encode(uint256[])) — identical to:
    ///   cast abi-encode 'f(uint256[])' '[rows,cols]' | cut -c3-
    pub async fn register_model(
        &self,
        model_name:    &str,
        model_version: &str,
        ipfs_cid:      &str,
        input_shape:   &[u64],
    ) -> Result<String, SettlerError> {
        use alloy::sol_types::SolValue;

        let model_id         = Self::derive_model_id(model_name, model_version);
        let shape_u256: Vec<U256> = input_shape.iter().map(|&x| U256::from(x)).collect();
        let input_shape_hash: FixedBytes<32> = keccak256(&shape_u256.abi_encode()).into();

        let signer   = self.build_signer()?;
        let wallet   = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect(&self.config.rpc_url)
            .await
            .map_err(|e| SettlerError::RpcError(e.to_string()))?;
        let address  = Address::from_str(&self.config.contract_address)
            .map_err(|e| SettlerError::InvalidAddress(e.to_string()))?;
        let contract = InferenceVerifier::new(address, &provider);

        let already: bool = contract
            .isRegisteredModel(model_id)
            .call()
            .await
            .map_err(|e: alloy::contract::Error| SettlerError::RpcError(e.to_string()))?;

        if already {
            warn!(model_name, model_id = %hex::encode(model_id), "model already registered, skipping");
            return Ok("already-registered".to_string());
        }

        info!(
            model_name, model_version,
            model_id         = %hex::encode(model_id),
            input_shape_hash = %hex::encode(input_shape_hash),
            %ipfs_cid,
            "registering model on-chain"
        );

        let tx = contract
            .registerModel(model_id, ipfs_cid.to_string(), input_shape_hash)
            .send()
            .await
            .map_err(|e: alloy::contract::Error| SettlerError::RpcError(e.to_string()))?;

        let tx_hash = *tx.tx_hash();
        info!(%tx_hash, "registerModel tx submitted, waiting for confirmation");

        let receipt = tokio::time::timeout(
            Duration::from_secs(self.config.tx_timeout_secs),
            tx.get_receipt(),
        )
        .await
        .map_err(|_| SettlerError::TxTimeout(self.config.tx_timeout_secs))?
        .map_err(|e| SettlerError::RpcError(e.to_string()))?;

        if !receipt.status() {
            return Err(SettlerError::TxReverted(format!("{tx_hash}")));
        }

        info!(
            tx_hash      = %tx_hash,
            block_number = ?receipt.block_number,
            gas_used     = receipt.gas_used,
            model_name,
            "model registered on-chain"
        );

        Ok(format!("{tx_hash:#x}"))
    }

    // ── Private: bridge submission ────────────────────────────────────────────
    //
    // Calls InferenceBridge.verifyAndBridge() on Eth Sepolia.
    // Payable — msg.value covers the StarkNet L1→L2 messaging fee
    // (SETTLER_STARKNET_BRIDGE_FEE_WEI, default 0.03 ETH on Sepolia).

    async fn bridge_submit(
        &self,
        proof_path:    &str,
        model_name:    &str,
        model_version: &str,
        inference_id:  &str,
    ) -> Result<String, SettlerError> {
        let parsed = ParsedProof::from_file(proof_path)?;

        let model_hash: FixedBytes<32> = Self::derive_model_id(model_name, model_version);

        // inference_id is a 0x-prefixed 32-byte hex string (padded from 16 raw bytes)
        let inference_id_bytes: FixedBytes<32> = {
            let h     = inference_id.trim_start_matches("0x");
            let bytes = hex::decode(h)
                .map_err(|e| SettlerError::RpcError(format!("invalid inference_id: {e}")))?;
            // Left-pad to 32 bytes for FixedBytes<32> — value is always ≤ 128 bits
            let mut padded = [0u8; 32];
            let offset = 32usize.saturating_sub(bytes.len());
            padded[offset..].copy_from_slice(&bytes[..bytes.len().min(32)]);
            FixedBytes::<32>::from(padded)
        };

        let signer   = self.build_signer()?;
        let wallet   = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect(&self.config.rpc_url)
            .await
            .map_err(|e| SettlerError::RpcError(e.to_string()))?;
        let address  = Address::from_str(&self.config.eth_sepolia_inference_bridge)
            .map_err(|e| SettlerError::InvalidAddress(e.to_string()))?;
        let contract = InferenceBridge::new(address, &provider);

        // Replay guard
        let already: bool = contract
            .isVerified(inference_id_bytes)
            .call()
            .await
            .map_err(|e: alloy::contract::Error| SettlerError::RpcError(e.to_string()))?;

        if already {
            warn!(inference_id, "inferenceId already bridged to StarkNet, skipping");
            return Ok("already-verified".to_string());
        }

        info!(
            model_name, model_version,
            model_hash  = %hex::encode(model_hash),
            inference_id,
            proof_bytes = parsed.proof.len(),
            "calling InferenceBridge.verifyAndBridge()"
        );

        let fee_wei = U256::from(self.config.starknet_bridge_fee_wei);

        let tx = contract
            .verifyAndBridge(
                parsed.proof.into(),
                parsed.instances,
                inference_id_bytes,
                model_hash,
            )
            .value(fee_wei)
            .send()
            .await
            .map_err(|e: alloy::contract::Error| SettlerError::RpcError(e.to_string()))?;

        let tx_hash = *tx.tx_hash();
        info!(%tx_hash, "verifyAndBridge tx submitted, waiting for confirmation");

        let receipt = tokio::time::timeout(
            Duration::from_secs(self.config.tx_timeout_secs),
            tx.get_receipt(),
        )
        .await
        .map_err(|_| SettlerError::TxTimeout(self.config.tx_timeout_secs))?
        .map_err(|e| SettlerError::RpcError(e.to_string()))?;

        if !receipt.status() {
            return Err(SettlerError::TxReverted(format!("{tx_hash}")));
        }

        info!(
            tx_hash      = %tx_hash,
            block_number = ?receipt.block_number,
            gas_used     = receipt.gas_used,
            inference_id,
            "bridge tx confirmed"
        );

        Ok(format!("{tx_hash:#x}"))
    }

    // ── Private: StarkNet polling ─────────────────────────────────────────────

    async fn poll_starknet_settlement(&self, inference_id: &str) -> Result<(), SettlerError> {
        use starknet::{
            core::types::{BlockId, BlockTag, FunctionCall, Felt},
            providers::{JsonRpcClient, Provider, Url, jsonrpc::HttpTransport},
        };

        let url = Url::parse(&self.config.starknet_rpc)
            .map_err(|e| SettlerError::ConfigError(format!("invalid STARKNET_RPC: {e}")))?;
        let provider = JsonRpcClient::new(HttpTransport::new(url));

        let contract = Felt::from_hex(&self.config.starknet_inference_verifier)
            .map_err(|e| SettlerError::ConfigError(format!("invalid STARKNET_INFERENCE_VERIFIER: {e}")))?;

        let selector = starknet::core::utils::get_selector_from_name("is_inference_verified")
            .map_err(|e| SettlerError::ConfigError(format!("selector error: {e}")))?;

        // inference_id is ≤ 128 bits so it fits directly in a Felt
        let id_felt = Felt::from_hex(inference_id)
            .map_err(|e| SettlerError::ConfigError(format!("invalid inference_id hex: {e}")))?;

        let interval = Duration::from_secs(self.config.starknet_poll_interval_secs);
        let max      = self.config.starknet_max_poll_attempts;

        for attempt in 0..max {
            tokio::time::sleep(interval).await;

            let result = provider
                .call(
                    FunctionCall {
                        contract_address:     contract,
                        entry_point_selector: selector,
                        calldata:             vec![id_felt],
                    },
                    BlockId::Tag(BlockTag::Latest),
                )
                .await;

            match result {
                Ok(ret) if !ret.is_empty() && ret[0] != Felt::ZERO => {
                    return Ok(());
                }
                Ok(_) => {
                    warn!(attempt, "StarkNet settlement not yet confirmed, retrying");
                }
                Err(e) => {
                    warn!(attempt, "StarkNet poll error: {e}, retrying");
                }
            }
        }

        Err(SettlerError::StarknetTimeout {
            attempts:      max,
            interval_secs: self.config.starknet_poll_interval_secs,
        })
    }

    // ── Private: helpers ──────────────────────────────────────────────────────

    /// keccak256(model_name || model_version) truncated to 128 bits.
    ///
    /// The high 16 bytes are zeroed so the uint256 value always fits within
    /// the StarkNet felt252 field (max ~2^251). Passed as modelHash in both
    /// the EVM payload and the StarkNet L1→L2 message.
    fn derive_model_id(model_name: &str, model_version: &str) -> FixedBytes<32> {
        let mut buf = Vec::with_capacity(model_name.len() + model_version.len());
        buf.extend_from_slice(model_name.as_bytes());
        buf.extend_from_slice(model_version.as_bytes());
        let hash = keccak256(&buf);
        // Zero the high 16 bytes — keep only the low 128 bits
        let mut truncated = [0u8; 32];
        truncated[16..].copy_from_slice(&hash[16..]);
        FixedBytes::<32>::from(truncated)
    }

    /// keccak256(input_data || output_data) truncated to 128 bits.
    ///
    /// Returns a 0x-prefixed hex string of the full 32-byte value with the
    /// high 16 bytes zeroed. The uint256 value is therefore always < 2^128,
    /// safely within felt252 range. Used as the replay key in both
    /// InferenceBridge.sol and InferenceVerifier.cairo.
    fn derive_inference_id(input_data: &[u8], output_data: &[u8]) -> String {
        let mut buf = Vec::with_capacity(input_data.len() + output_data.len());
        buf.extend_from_slice(input_data);
        buf.extend_from_slice(output_data);
        let hash = keccak256(&buf);
        // Zero the high 16 bytes, encode the full 32 bytes as hex
        // Result is always a valid felt252: value = hash[16..32] < 2^128
        let mut truncated = [0u8; 32];
        truncated[16..].copy_from_slice(&hash[16..]);
        format!("0x{}", hex::encode(truncated))
    }

    fn build_signer(&self) -> Result<PrivateKeySigner, SettlerError> {
        let key = self.config.private_key.trim_start_matches("0x");
        PrivateKeySigner::from_str(key)
            .map_err(|e| SettlerError::InvalidKey(e.to_string()))
    }
}