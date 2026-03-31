// SPDX: inference_verifier.cairo
//
// StarkNet settlement consumer for Elenxis cross-chain inference verification.
//
// Role:
//   Accepts verified inference results from InferenceBridge.sol on EVM via
//   the canonical StarkNet L1→L2 messaging system. Writes a tamper-proof
//   InferenceRecord to storage and emits a StarkNet event.
//
//   This contract does ZERO cryptographic verification. The KZG pairing
//   check already ran on EVM. Trust comes from:
//     (a) StarkNet core only delivering messages from the exact L1 address
//         registered in `l1_verifier_whitelist`
//     (b) replay protection via `verified_inferences` map
//
// Entry points:
//   #[l1_handler]  consume_inference_result  — called by StarkNet core on L1→L2 message
//   #[view]        is_inference_verified      — read settlement status
//   #[view]        get_inference_record       — read full record
//   #[external]    add_l1_verifier            — owner: whitelist a new L1 bridge address
//   #[external]    remove_l1_verifier         — owner: remove an L1 bridge address

use starknet::ContractAddress;
use starknet::EthAddress;

// ── Storage types ─────────────────────────────────────────────────────────────

#[derive(Drop, Serde, starknet::Store, Copy)]
pub struct InferenceRecord {
    pub inference_id: felt252,
    pub model_hash:   felt252,
    pub submitter:    felt252,    // EthAddress packed as felt252
    pub verified_at:  u64,
}

// ── Interface ─────────────────────────────────────────────────────────────────

#[starknet::interface]
pub trait IInferenceVerifier<TContractState> {
    fn is_inference_verified(self: @TContractState, inference_id: felt252) -> bool;
    fn get_inference_record(self: @TContractState, inference_id: felt252) -> InferenceRecord;
    fn get_owner(self: @TContractState) -> ContractAddress;
    fn is_l1_verifier(self: @TContractState, l1_address: EthAddress) -> bool;
    fn add_l1_verifier(ref self: TContractState, l1_address: EthAddress);
    fn remove_l1_verifier(ref self: TContractState, l1_address: EthAddress);
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[starknet::contract]
pub mod InferenceVerifier {
    use super::{InferenceRecord, IInferenceVerifier};
    use starknet::ContractAddress;
    use starknet::EthAddress;
    use starknet::get_caller_address;
    use starknet::storage::*;

    // ── Storage ───────────────────────────────────────────────────────────────

    #[storage]
    struct Storage {
        /// Settled inference records keyed by inference_id (felt252).
        verified_inferences:   Map<felt252, InferenceRecord>,

        /// Whitelist of L1 bridge addresses allowed to send messages.
        /// Supports both Eth Sepolia and Base Sepolia InferenceBridge.sol deployments.
        l1_verifier_whitelist: Map<EthAddress, bool>,

        /// Contract owner — can manage the L1 whitelist.
        owner:                 ContractAddress,
    }

    // ── Events ────────────────────────────────────────────────────────────────

    #[event]
    #[derive(Drop, starknet::Event)]
    pub enum Event {
        InferenceVerified: InferenceVerified,
        L1VerifierAdded:   L1VerifierAdded,
        L1VerifierRemoved: L1VerifierRemoved,
    }

    /// Emitted when an inference result is recorded from L1.
    #[derive(Drop, starknet::Event)]
    pub struct InferenceVerified {
        #[key]
        pub inference_id: felt252,
        pub model_hash:   felt252,
        pub submitter:    felt252,
        pub verified_at:  u64,
    }

    #[derive(Drop, starknet::Event)]
    pub struct L1VerifierAdded {
        #[key]
        pub l1_address: EthAddress,
    }

    #[derive(Drop, starknet::Event)]
    pub struct L1VerifierRemoved {
        #[key]
        pub l1_address: EthAddress,
    }

    // ── Constructor ───────────────────────────────────────────────────────────

    #[constructor]
    fn constructor(
        ref self: ContractState,
        owner: ContractAddress,
        // Initial L1 bridge address to whitelist (e.g. Eth Sepolia InferenceBridge.sol).
        // Pass additional addresses via add_l1_verifier() after deployment.
        initial_l1_verifier: EthAddress,
    ) {
        self.owner.write(owner);
        self.l1_verifier_whitelist.write(initial_l1_verifier, true);
        self.emit(L1VerifierAdded { l1_address: initial_l1_verifier });
    }

    // ── L1 handler ────────────────────────────────────────────────────────────

    /// Called by StarkNet core when an L1→L2 message arrives from InferenceBridge.sol.
    ///
    /// `from_address` is injected by StarkNet core — it is the msg.sender of the
    /// sendMessageToL2() call on L1, i.e. the InferenceBridge.sol contract address.
    ///
    /// Payload layout (must match InferenceBridge.sol verifyAndBridge()):
    ///   payload[0] = inference_id  (bytes32 cast to felt252)
    ///   payload[1] = model_hash    (bytes32 cast to felt252)
    ///   payload[2] = timestamp     (uint256, fits in u64)
    ///   payload[3] = submitter     (address cast to uint160 cast to felt252)
    #[l1_handler]
    fn consume_inference_result(
        ref self: ContractState,
        from_address: felt252,
        inference_id: felt252,
        model_hash:   felt252,
        timestamp:    u64,
        submitter:    felt252,
    ) {
        // ── 1. Validate L1 sender ─────────────────────────────────────────────
        // from_address is the EVM address of InferenceBridge.sol. It must be in
        // our whitelist — this is the only security check we need.
        let sender: EthAddress = from_address.try_into().expect('unauthorized L1 sender');
        assert!(
            self.l1_verifier_whitelist.read(sender),
            "unauthorized L1 sender"
        );

        // ── 2. Replay protection ──────────────────────────────────────────────
        let existing = self.verified_inferences.read(inference_id);
        assert!(existing.inference_id == 0, "already recorded");

        // ── 3. Write record ───────────────────────────────────────────────────
        let record = InferenceRecord {
            inference_id,
            model_hash,
            submitter,
            verified_at: timestamp,
        };
        self.verified_inferences.write(inference_id, record);

        // ── 4. Emit event ─────────────────────────────────────────────────────
        self.emit(InferenceVerified {
            inference_id,
            model_hash,
            submitter,
            verified_at: timestamp,
        });
    }

    // ── View + external impl ──────────────────────────────────────────────────

    #[abi(embed_v0)]
    impl InferenceVerifierImpl of IInferenceVerifier<ContractState> {

        /// Returns true if inference_id has a settled record.
        fn is_inference_verified(self: @ContractState, inference_id: felt252) -> bool {
            self.verified_inferences.read(inference_id).inference_id != 0
        }

        /// Returns the full InferenceRecord for a settled inference.
        /// Returns a zero-initialised struct if not found — callers should
        /// check is_inference_verified() first.
        fn get_inference_record(self: @ContractState, inference_id: felt252) -> InferenceRecord {
            self.verified_inferences.read(inference_id)
        }

        fn get_owner(self: @ContractState) -> ContractAddress {
            self.owner.read()
        }

        fn is_l1_verifier(self: @ContractState, l1_address: EthAddress) -> bool {
            self.l1_verifier_whitelist.read(l1_address)
        }

        /// Owner: add an L1 bridge address to the whitelist.
        /// Call this to add the Base Sepolia InferenceBridge.sol address.
        fn add_l1_verifier(ref self: ContractState, l1_address: EthAddress) {
            self._assert_owner();
            self.l1_verifier_whitelist.write(l1_address, true);
            self.emit(L1VerifierAdded { l1_address });
        }

        /// Owner: remove an L1 bridge address from the whitelist.
        fn remove_l1_verifier(ref self: ContractState, l1_address: EthAddress) {
            self._assert_owner();
            self.l1_verifier_whitelist.write(l1_address, false);
            self.emit(L1VerifierRemoved { l1_address });
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    #[generate_trait]
    impl InternalImpl of InternalTrait {
        fn _assert_owner(self: @ContractState) {
            assert!(get_caller_address() == self.owner.read(), "not owner");
        }
    }
}
