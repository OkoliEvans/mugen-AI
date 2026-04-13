// crates/settler/src/settler.rs

use alloy::{
    network::EthereumWallet,
    primitives::{keccak256, Address, Bytes, FixedBytes, U256},
    providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolValue,
};
use std::str::FromStr;
use std::time::Duration;
use tracing::{info, warn};

use crate::{config::SettlerConfig, error::SettlerError};

// ── Contract bindings — matches deployed InferenceVerifier.sol on HashKey ─────
//
// submitProof(bytes proofBytes, bytes publicValues)
// Uses the full ABI from the deployed contract.
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    InferenceVerifier,
    r#"[
        {
            "type": "function",
            "name": "submitProof",
            "inputs": [
                { "name": "proofBytes",    "type": "bytes", "internalType": "bytes" },
                { "name": "publicValues",  "type": "bytes", "internalType": "bytes" }
            ],
            "outputs": [],
            "stateMutability": "nonpayable"
        },
        {
            "type": "function",
            "name": "registerModel",
            "inputs": [
                { "name": "modelId",        "type": "bytes32", "internalType": "bytes32" },
                { "name": "ipfsCid",        "type": "string",  "internalType": "string" },
                { "name": "inputShapeHash", "type": "bytes32", "internalType": "bytes32" }
            ],
            "outputs": [],
            "stateMutability": "nonpayable"
        },
        {
            "type": "function",
            "name": "computeModelId",
            "inputs": [
                { "name": "name",    "type": "string", "internalType": "string" },
                { "name": "version", "type": "string", "internalType": "string" }
            ],
            "outputs": [
                { "name": "", "type": "bytes32", "internalType": "bytes32" }
            ],
            "stateMutability": "pure"
        },
        {
            "type": "function",
            "name": "isVerified",
            "inputs": [
                { "name": "outputHash", "type": "bytes32", "internalType": "bytes32" }
            ],
            "outputs": [
                { "name": "", "type": "bool", "internalType": "bool" }
            ],
            "stateMutability": "view"
        },
        {
            "type": "function",
            "name": "isRegisteredModel",
            "inputs": [
                { "name": "modelId", "type": "bytes32", "internalType": "bytes32" }
            ],
            "outputs": [
                { "name": "", "type": "bool", "internalType": "bool" }
            ],
            "stateMutability": "view"
        }
    ]"#
);

/// Submits SP1 proofs directly to InferenceVerifier.sol on HashKey testnet.
///
/// Settlement flow (no bridging):
///   1. Read proof file written by prover_manager (bincode-serialized SP1ProofWithPublicValues)
///   2. Extract proofBytes + publicValues
///   3. Call InferenceVerifier.submitProof() on HashKey testnet
///   4. Contract verifies the SP1 proof via ISP1Verifier and emits InferenceVerified
pub struct Settler {
    config: SettlerConfig,
}

impl Settler {
    pub fn new(config: SettlerConfig) -> Self {
        Self { config }
    }

    // ── Public API ────────────────────────────────────────────────────────────

    pub async fn submit(
        &self,
        proof_path: &str,
        model_name: &str,
        model_version: &str,
        _input_data: &[u8],
        _output_data: &[u8],
    ) -> Result<String, SettlerError> {
        info!(
            model_name,
            "submitting proof to InferenceVerifier on HashKey testnet"
        );

        let proof_bytes = tokio::fs::read(proof_path).await.map_err(|e| {
            SettlerError::ConfigError(format!("proof file not found at {proof_path}: {e}"))
        })?;

        let proof: sp1_sdk::SP1ProofWithPublicValues = bincode::deserialize(&proof_bytes)
            .map_err(|e| SettlerError::ConfigError(format!("failed to deserialize proof: {e}")))?;

        let raw_proof: Bytes = proof.bytes().into();
        let public_values: Bytes = proof.public_values.to_vec().into();
        let output_hash: FixedBytes<32> = keccak256(&public_values);

        let provider = self.build_provider().await?;
        let address = Address::from_str(&self.config.contract_address)
            .map_err(|e| SettlerError::InvalidAddress(e.to_string()))?;
        let contract = InferenceVerifier::new(address, &provider);

        let already = contract
            .isVerified(output_hash)
            .call()
            .await
            .map_err(|e| SettlerError::RpcError(e.to_string()))?;

        if already {
            warn!(
                model_name,
                output_hash = %hex::encode(output_hash),
                "proof already verified on HashKey — skipping"
            );
            return Ok("already-verified".to_string());
        }

        info!(
            model_name, model_version,
            output_hash   = %hex::encode(output_hash),
            proof_len     = raw_proof.len(),
            pubvalues_len = public_values.len(),
            "calling InferenceVerifier.submitProof()"
        );

        let tx = contract
            .submitProof(raw_proof, public_values)
            .send()
            .await
            .map_err(|e| SettlerError::RpcError(e.to_string()))?;

        let tx_hash = *tx.tx_hash();
        info!(%tx_hash, "submitProof tx submitted — waiting for confirmation");

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
            "proof settled on HashKey testnet ✓"
        );

        Ok(format!("{tx_hash:#x}"))
    }

    pub async fn register_model(
        &self,
        model_name: &str,
        model_version: &str,
        ipfs_cid: &str,
        input_shape: &[u64],
        weight_bytes: &[u8],
    ) -> Result<String, SettlerError> {
        let shape_u256: Vec<U256> = input_shape.iter().map(|&x| U256::from(x)).collect();
        let input_shape_hash: FixedBytes<32> = keccak256(&shape_u256.abi_encode()).into();

        let model_id: FixedBytes<32> = {
            use sha2::{Digest, Sha256};
            let hash: [u8; 32] = Sha256::digest(weight_bytes).into();
            FixedBytes::from(hash)
        };

        let provider = self.build_provider().await?;
        let address = Address::from_str(&self.config.contract_address)
            .map_err(|e| SettlerError::InvalidAddress(e.to_string()))?;
        let contract = InferenceVerifier::new(address, &provider);

        let already = contract
            .isRegisteredModel(model_id)
            .call()
            .await
            .map_err(|e| SettlerError::RpcError(e.to_string()))?;

        if already {
            warn!(
                model_name,
                model_id = %hex::encode(model_id),
                "model already registered — skipping"
            );
            return Ok("already-registered".to_string());
        }

        info!(
            model_name, model_version,
            model_id         = %hex::encode(model_id),
            input_shape_hash = %hex::encode(input_shape_hash),
            %ipfs_cid,
            "registering model on HashKey testnet"
        );

        let tx = contract
            .registerModel(model_id, ipfs_cid.to_string(), input_shape_hash)
            .send()
            .await
            .map_err(|e| SettlerError::RpcError(e.to_string()))?;

        let tx_hash = *tx.tx_hash();
        info!(%tx_hash, "registerModel tx submitted — waiting for confirmation");

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
            "model registered on HashKey testnet ✓"
        );

        Ok(format!("{tx_hash:#x}"))
    }

    pub async fn submit_aggregated(
        &self,
        _proof_path: &str,
        _output_hashes: &[[u8; 32]],
    ) -> Result<String, SettlerError> {
        Err(SettlerError::ConfigError(
            "aggregated settlement is not implemented in Settler yet".to_string(),
        ))
    }

    // ── Private ───────────────────────────────────────────────────────────────

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
}
