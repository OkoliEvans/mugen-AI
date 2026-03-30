use alloy::{
    network::EthereumWallet,
    primitives::{Address, FixedBytes, keccak256},
    providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
};
use std::str::FromStr;
use tracing::{info, warn};

use crate::{
    config::SettlerConfig,
    contract::{InferenceVerifier, ParsedProof},
    error::SettlerError,
};

/// Submits ZK proofs to the on-chain InferenceVerifier contract.
pub struct Settler {
    config: SettlerConfig,
}

impl Settler {
    pub fn new(config: SettlerConfig) -> Self {
        Self { config }
    }

    /// Submit a proof for a completed job.
    ///
    /// # Arguments
    /// * `proof_path`     — path to the EZKL proof.json on disk
    /// * `model_name`     — human-readable model name (e.g. "tiny_mlp_v1")
    /// * `model_version`  — semver string (e.g. "0.1.0")
    /// * `input_data`     — raw input bytes (hashed locally, not sent on-chain)
    /// * `output_data`    — raw output bytes (hashed locally, stored as attestation key)
    ///
    /// # Returns
    /// The transaction hash as a hex string, or "already-verified" if already settled.
    ///
    /// # modelId derivation
    /// Matches the contract's `computeModelId(name, version)`:
    ///   keccak256(abi.encodePacked(name, version))
    /// which in Rust is keccak256([name_bytes, version_bytes].concat()).
    pub async fn submit(
        &self,
        proof_path:    &str,
        model_name:    &str,
        model_version: &str,
        input_data:    &[u8],
        output_data:   &[u8],
    ) -> Result<String, SettlerError> {
        // --- Parse proof file ---
        let parsed = ParsedProof::from_file(proof_path)?;

        // --- Derive hashes ---
        //
        // modelId must match InferenceVerifier.computeModelId(name, version):
        //   keccak256(abi.encodePacked(name, version))
        // abi.encodePacked for two strings is just their raw bytes concatenated.
        let mut model_id_input = Vec::with_capacity(model_name.len() + model_version.len());
        model_id_input.extend_from_slice(model_name.as_bytes());
        model_id_input.extend_from_slice(model_version.as_bytes());
        let model_id_bytes: FixedBytes<32> = keccak256(&model_id_input).into();

        let input_hash:  FixedBytes<32> = keccak256(input_data).into();
        let output_hash: FixedBytes<32> = keccak256(output_data).into();

        // --- Build signer + wallet + provider ---
        let signer  = self.build_signer()?;
        let wallet  = EthereumWallet::from(signer);

        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect(&self.config.rpc_url)
            .await
            .map_err(|e| SettlerError::RpcError(e.to_string()))?;

        // --- Build contract instance ---
        let address = Address::from_str(&self.config.contract_address)
            .map_err(|e| SettlerError::InvalidAddress(e.to_string()))?;

        let contract = InferenceVerifier::new(address, &provider);

        // --- Check if already verified (avoid wasting gas) ---
        let already: bool = contract
            .isVerified(output_hash)
            .call()
            .await
            .map_err(|e: alloy::contract::Error| SettlerError::RpcError(e.to_string()))?;

        if already {
            warn!(
                output_hash = %hex::encode(output_hash),
                "already verified on-chain, skipping"
            );
            return Ok("already-verified".to_string());
        }

        info!(
            model_name,
            model_version,
            model_id    = %hex::encode(model_id_bytes),
            output_hash = %hex::encode(output_hash),
            instances   = parsed.instances.len(),
            proof_bytes = parsed.proof.len(),
            "submitting proof on-chain"
        );

        // --- Send transaction ---
        let tx = contract
            .submitProof(
                parsed.proof.into(),
                parsed.instances,
                model_id_bytes,
                input_hash,
                output_hash,
            )
            .send()
            .await
            .map_err(|e: alloy::contract::Error| SettlerError::RpcError(e.to_string()))?;

        let tx_hash = *tx.tx_hash();

        info!(tx_hash = %tx_hash, "transaction submitted, waiting for confirmation");

        // --- Wait for receipt ---
        let receipt = tokio::time::timeout(
            std::time::Duration::from_secs(self.config.tx_timeout_secs),
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
            "proof settled on-chain"
        );

        Ok(format!("{tx_hash:#x}"))
    }

    /// Register a model on the InferenceVerifier contract.
    ///
    /// # Arguments
    /// * `model_name`    — human-readable model name (e.g. "tiny_mlp_v1")
    /// * `model_version` — semver string (e.g. "0.1.0")
    /// * `ipfs_cid`      — IPFS CID of the pinned model artifact
    /// * `input_shape`   — model input dimensions (e.g. [1, 4])
    ///
    /// # modelId derivation
    /// Matches InferenceVerifier.computeModelId(name, version):
    ///   keccak256(abi.encodePacked(name, version))
    ///
    /// # inputShapeHash derivation
    /// Matches the deploy script's keccak256(vm.envBytes("MODEL_INPUT_SHAPE"))
    /// where MODEL_INPUT_SHAPE = cast abi-encode 'f(uint256[])' '[1,4]' | cut -c3-
    /// Alloy's SolValue::abi_encode on Vec<U256> produces the identical encoding.
    ///
    /// # Returns
    /// The transaction hash as a hex string.
    pub async fn register_model(
        &self,
        model_name:    &str,
        model_version: &str,
        ipfs_cid:      &str,
        input_shape:   &[u64],
    ) -> Result<String, SettlerError> {
        use alloy::sol_types::SolValue;
        use alloy::primitives::U256;

        // --- Derive modelId: keccak256(abi.encodePacked(name, version)) ---
        let mut model_id_input = Vec::with_capacity(model_name.len() + model_version.len());
        model_id_input.extend_from_slice(model_name.as_bytes());
        model_id_input.extend_from_slice(model_version.as_bytes());
        let model_id: FixedBytes<32> = keccak256(&model_id_input).into();

        // --- Derive inputShapeHash: keccak256(abi_encode(uint256[])) ---
        // Must match: cast abi-encode 'f(uint256[])' '[rows,cols]' | cut -c3-
        let shape_u256: Vec<U256> = input_shape.iter().map(|&x| U256::from(x)).collect();
        let shape_encoded = shape_u256.abi_encode();
        let input_shape_hash: FixedBytes<32> = keccak256(&shape_encoded).into();

        // --- Build provider + contract ---
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

        // --- Guard: skip if already registered ---
        let already: bool = contract
            .isRegisteredModel(model_id)
            .call()
            .await
            .map_err(|e: alloy::contract::Error| SettlerError::RpcError(e.to_string()))?;

        if already {
            warn!(
                model_name,
                model_id = %hex::encode(model_id),
                "model already registered on-chain, skipping"
            );
            return Ok("already-registered".to_string());
        }

        info!(
            model_name,
            model_version,
            model_id         = %hex::encode(model_id),
            input_shape_hash = %hex::encode(input_shape_hash),
            %ipfs_cid,
            "registering model on-chain"
        );

        // --- Send registerModel() tx ---
        let tx = contract
            .registerModel(model_id, ipfs_cid.to_string(), input_shape_hash)
            .send()
            .await
            .map_err(|e: alloy::contract::Error| SettlerError::RpcError(e.to_string()))?;

        let tx_hash = *tx.tx_hash();
        info!(tx_hash = %tx_hash, "registerModel tx submitted, waiting for confirmation");

        let receipt = tokio::time::timeout(
            std::time::Duration::from_secs(self.config.tx_timeout_secs),
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

    fn build_signer(&self) -> Result<PrivateKeySigner, SettlerError> {
        let key = self.config.private_key.trim_start_matches("0x");
        PrivateKeySigner::from_str(key)
            .map_err(|e| SettlerError::InvalidKey(e.to_string()))
    }
}