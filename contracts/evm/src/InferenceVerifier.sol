// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";
import {
    ReentrancyGuard
} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {ISP1Verifier} from "@sp1-contracts/ISP1Verifier.sol";

/// @title  InferenceVerifier
/// @notice Registry and attestation layer wrapping the SP1 universal verifier.
///
///         Two proof submission paths:
///
///         1. submitProof()           — single inference proof (unchanged).
///                                      publicValues: abi.encode(modelId, inputHash, outputHash)
///
///         2. submitAggregatedProof() — aggregated proof covering N inferences.
///                                      publicValues: abi.encode(merkleRoot, batchSize)
///                                      outputHashes: the N individual output hashes whose
///                                                    merkle root was committed by the aggregator.
///
///         The aggregated path uses a separate vkey (aggregationVKey) that identifies
///         the aggregator-guest program, not the inference-guest program.
contract InferenceVerifier is Ownable2Step, Pausable, ReentrancyGuard {
    // -------------------------------------------------------------------------
    // Types
    // -------------------------------------------------------------------------

    struct Attestation {
        bytes32 modelId; // sha256 of model weights — committed by SP1 guest
        bytes32 inputHash; // sha256 of raw input — committed by SP1 guest
        address settler; // which settler submitted this proof
        uint48 timestamp; // block.timestamp at submission
    }

    struct Model {
        bytes32 ipfsCidHash; // keccak256 of the IPFS CID string
        bytes32 inputShapeHash; // keccak256 of ABI-encoded input shape
        address registeredBy;
        uint48 registeredAt;
    }

    // -------------------------------------------------------------------------
    // Storage
    // -------------------------------------------------------------------------

    /// @notice SP1VerifierGateway — universal verifier deployed by Succinct.
    ISP1Verifier public sp1Verifier;

    /// @notice SP1 vkey for the single inference guest program.
    bytes32 public inferenceVKey;

    /// @notice SP1 vkey for the aggregator guest program.
    ///         Set via setAggregationVKey() after deploying the aggregator ELF.
    ///         Zero means aggregated proof submission is disabled.
    bytes32 public aggregationVKey;

    /// @notice Settler whitelist.
    mapping(address => bool) public isSettler;

    /// @notice outputHash → attestation. Permanent once written.
    mapping(bytes32 => Attestation) private _attestations;

    /// @notice outputHash → verified flag. Separate for cheap O(1) lookups.
    mapping(bytes32 => bool) public isVerified;

    /// @notice merkleRoot → batch settlement record.
    mapping(bytes32 => BatchSettlement) private _batches;

    /// @notice merkleRoot → settled flag.
    mapping(bytes32 => bool) public isBatchSettled;

    /// @notice modelId → model registration record.
    mapping(bytes32 => Model) private _models;

    /// @notice modelId → registered flag.
    mapping(bytes32 => bool) public isRegisteredModel;

    // -------------------------------------------------------------------------
    // Types (batch)
    // -------------------------------------------------------------------------

    struct BatchSettlement {
        bytes32 merkleRoot; // sha256 tree over all output_hashes in batch
        uint32 batchSize; // number of proofs aggregated
        address settler;
        uint48 timestamp;
    }

    // -------------------------------------------------------------------------
    // Events
    // -------------------------------------------------------------------------

    event InferenceVerified(
        bytes32 indexed outputHash,
        bytes32 indexed modelId,
        address indexed settler,
        uint256 timestamp
    );

    event BatchVerified(
        bytes32 indexed merkleRoot,
        uint32 batchSize,
        address indexed settler,
        uint256 timestamp
    );

    event OutputHashRegistered(
        bytes32 indexed merkleRoot,
        bytes32 indexed outputHash,
        uint32 index
    );

    event ModelRegistered(
        bytes32 indexed modelId,
        string ipfsCid,
        bytes32 indexed ipfsCidHash,
        bytes32 inputShapeHash,
        address indexed registeredBy
    );

    event SettlerUpdated(address indexed settler, bool approved);
    event VerifierUpgraded(
        address indexed oldVerifier,
        address indexed newVerifier
    );
    event VKeyUpdated(bytes32 indexed oldVKey, bytes32 indexed newVKey);
    event AggregationVKeyUpdated(
        bytes32 indexed oldVKey,
        bytes32 indexed newVKey
    );

    // -------------------------------------------------------------------------
    // Errors
    // -------------------------------------------------------------------------

    error NotSettler(address caller);
    error AlreadyVerified(bytes32 outputHash);
    error BatchAlreadySettled(bytes32 merkleRoot);
    error InvalidProof();
    error ZeroAddress();
    error ZeroVKey();
    error AggregationVKeyNotSet();
    error ModelAlreadyRegistered(bytes32 modelId);
    error ModelNotRegistered(bytes32 modelId);
    error EmptyString();
    error EmptyBatch();
    error MerkleRootMismatch(bytes32 expected, bytes32 actual);

    // -------------------------------------------------------------------------
    // Constructor
    // -------------------------------------------------------------------------

    /// @param _sp1Verifier   Address of the SP1VerifierGateway.
    /// @param _inferenceVKey The SP1 vkey for the inference guest program.
    /// @param _initialOwner  Address that will own this contract.
    constructor(
        address _sp1Verifier,
        bytes32 _inferenceVKey,
        address _initialOwner
    ) Ownable(_initialOwner) {
        if (_sp1Verifier == address(0)) revert ZeroAddress();
        if (_inferenceVKey == bytes32(0)) revert ZeroVKey();

        sp1Verifier = ISP1Verifier(_sp1Verifier);
        inferenceVKey = _inferenceVKey;
    }

    // -------------------------------------------------------------------------
    // Core — Model Registration
    // -------------------------------------------------------------------------

    /// @notice Register a model on-chain after it has been pinned to IPFS.
    function registerModel(
        bytes32 modelId,
        string calldata ipfsCid,
        bytes32 inputShapeHash
    ) external onlyOwner {
        if (isRegisteredModel[modelId]) revert ModelAlreadyRegistered(modelId);
        if (bytes(ipfsCid).length == 0) revert EmptyString();

        bytes32 ipfsCidHash = keccak256(bytes(ipfsCid));

        _models[modelId] = Model({
            ipfsCidHash: ipfsCidHash,
            inputShapeHash: inputShapeHash,
            registeredBy: msg.sender,
            registeredAt: uint48(block.timestamp)
        });
        isRegisteredModel[modelId] = true;

        emit ModelRegistered(
            modelId,
            ipfsCid,
            ipfsCidHash,
            inputShapeHash,
            msg.sender
        );
    }

    // -------------------------------------------------------------------------
    // Core — Single Proof Submission (unchanged)
    // -------------------------------------------------------------------------

    /// @notice Submit an SP1 proof of correct single inference execution.
    /// @param proofBytes   Groth16 SP1 proof bytes.
    /// @param publicValues abi.encode(bytes32 modelId, bytes32 inputHash, bytes32 outputHash)
    function submitProof(
        bytes calldata proofBytes,
        bytes calldata publicValues
    ) external nonReentrant whenNotPaused {
        if (!isSettler[msg.sender]) revert NotSettler(msg.sender);

        (bytes32 modelId, bytes32 inputHash, bytes32 outputHash) = abi.decode(
            publicValues,
            (bytes32, bytes32, bytes32)
        );

        if (isVerified[outputHash]) revert AlreadyVerified(outputHash);

        // Delegate cryptographic verification to SP1VerifierGateway.
        sp1Verifier.verifyProof(inferenceVKey, publicValues, proofBytes);

        _attestations[outputHash] = Attestation({
            modelId: modelId,
            inputHash: inputHash,
            settler: msg.sender,
            timestamp: uint48(block.timestamp)
        });
        isVerified[outputHash] = true;

        emit InferenceVerified(
            outputHash,
            modelId,
            msg.sender,
            block.timestamp
        );
    }

    // -------------------------------------------------------------------------
    // Core — Aggregated Proof Submission (new)
    // -------------------------------------------------------------------------

    /// @notice Submit one Groth16 proof covering N inference executions.
    ///
    /// @param proofBytes    Aggregated Groth16 proof bytes from aggregator-guest.
    /// @param publicValues  abi.encode(bytes32 merkleRoot, uint32 batchSize)
    ///                      Committed by aggregator-guest via sp1_zkvm::io::commit().
    /// @param outputHashes  The N individual output_hashes whose sha256 merkle root
    ///                      was committed inside the aggregated proof. Supplied by
    ///                      the settler off-chain — verified on-chain by recomputing
    ///                      the merkle root and comparing to the committed value.
    ///
    /// @dev  The aggregator-guest verifies each individual compressed inference proof
    ///       recursively inside the zkVM before committing the merkle root. This
    ///       contract only needs to verify the outer Groth16 proof and the merkle root.
    function submitAggregatedProof(
        bytes calldata proofBytes,
        bytes calldata publicValues,
        bytes32[] calldata outputHashes
    ) external nonReentrant whenNotPaused {
        if (!isSettler[msg.sender]) revert NotSettler(msg.sender);
        if (aggregationVKey == bytes32(0)) revert AggregationVKeyNotSet();
        if (outputHashes.length == 0) revert EmptyBatch();

        // Public values layout committed by aggregator-guest (36 bytes total):
        //   sp1_zkvm::io::commit(&root) → 32 bytes, big-endian bytes32
        //   sp1_zkvm::io::commit(&n)    →  4 bytes, u32 little-endian
        // Cannot use abi.decode — that expects 64 bytes (uint32 padded to 32).
        require(publicValues.length == 36, "invalid publicValues length");

        bytes32 merkleRoot = bytes32(publicValues[0:32]);

        uint32 batchSize;
        assembly {
            // Read 4 bytes at offset 32 from publicValues data.
            // calldataload reads 32 bytes left-aligned; our 4 bytes are
            // in the highest positions. Rust writes u32 little-endian:
            // byte[32]=LSB ... byte[35]=MSB
            let raw := calldataload(add(publicValues.offset, 32))
            let b0 := byte(0, raw)
            let b1 := byte(1, raw)
            let b2 := byte(2, raw)
            let b3 := byte(3, raw)
            batchSize := or(or(or(b0, shl(8, b1)), shl(16, b2)), shl(24, b3))
        }

        if (isBatchSettled[merkleRoot]) revert BatchAlreadySettled(merkleRoot);

        // Recompute merkle root from supplied outputHashes and verify it
        // matches what the aggregator-guest committed inside the proof.
        bytes32 recomputed = _merkleRoot(outputHashes);
        if (recomputed != merkleRoot)
            revert MerkleRootMismatch(merkleRoot, recomputed);

        // Verify the outer Groth16 aggregated proof against the aggregationVKey.
        sp1Verifier.verifyProof(aggregationVKey, publicValues, proofBytes);

        // Record the batch settlement.
        _batches[merkleRoot] = BatchSettlement({
            merkleRoot: merkleRoot,
            batchSize: batchSize,
            settler: msg.sender,
            timestamp: uint48(block.timestamp)
        });
        isBatchSettled[merkleRoot] = true;

        emit BatchVerified(merkleRoot, batchSize, msg.sender, block.timestamp);

        // Mark each individual output hash as verified and emit per-hash event.
        // This preserves isVerified() compatibility for existing integrations.
        for (uint256 i = 0; i < outputHashes.length; i++) {
            bytes32 h = outputHashes[i];
            if (!isVerified[h]) {
                isVerified[h] = true;
                emit OutputHashRegistered(merkleRoot, h, uint32(i));
            }
        }
    }

    // -------------------------------------------------------------------------
    // Views
    // -------------------------------------------------------------------------

    function getAttestation(
        bytes32 outputHash
    ) external view returns (Attestation memory) {
        return _attestations[outputHash];
    }

    function getBatchSettlement(
        bytes32 merkleRoot
    ) external view returns (BatchSettlement memory) {
        return _batches[merkleRoot];
    }

    function getModel(bytes32 modelId) external view returns (Model memory) {
        if (!isRegisteredModel[modelId]) revert ModelNotRegistered(modelId);
        return _models[modelId];
    }

    /// @notice Convenience: derive modelId from name + version.
    ///         Note: SP1 system uses sha256(weights_bytes) as modelId.
    ///         This helper is for registry lookups only.
    function computeModelId(
        string calldata name,
        string calldata version
    ) external pure returns (bytes32) {
        return keccak256(abi.encodePacked(name, version));
    }

    // -------------------------------------------------------------------------
    // Admin
    // -------------------------------------------------------------------------

    function setSettler(address settler, bool approved) external onlyOwner {
        if (settler == address(0)) revert ZeroAddress();
        isSettler[settler] = approved;
        emit SettlerUpdated(settler, approved);
    }

    /// @notice Set the aggregator-guest vkey. Required before submitAggregatedProof()
    ///         can be called. Get via: cargo prove vkey --elf aggregator-guest
    function setAggregationVKey(bytes32 newVKey) external onlyOwner {
        if (newVKey == bytes32(0)) revert ZeroVKey();
        bytes32 old = aggregationVKey;
        aggregationVKey = newVKey;
        emit AggregationVKeyUpdated(old, newVKey);
    }

    function upgradeVerifier(address newVerifier) external onlyOwner {
        if (newVerifier == address(0)) revert ZeroAddress();
        address old = address(sp1Verifier);
        sp1Verifier = ISP1Verifier(newVerifier);
        emit VerifierUpgraded(old, newVerifier);
    }

    function updateVKey(bytes32 newVKey) external onlyOwner {
        if (newVKey == bytes32(0)) revert ZeroVKey();
        bytes32 old = inferenceVKey;
        inferenceVKey = newVKey;
        emit VKeyUpdated(old, newVKey);
    }

    function pause() external onlyOwner {
        _pause();
    }
    function unpause() external onlyOwner {
        _unpause();
    }

    // -------------------------------------------------------------------------
    // Internal
    // -------------------------------------------------------------------------

    /// @dev Replicates the merkle_root() logic in aggregator/src/lib.rs exactly:
    ///      sha256(all hashes concatenated) for N > 1, or hashes[0] for N == 1.
    ///      Must stay in sync with the Rust implementation.
    function _merkleRoot(
        bytes32[] calldata hashes
    ) internal pure returns (bytes32) {
        if (hashes.length == 1) return hashes[0];
        bytes memory combined = new bytes(hashes.length * 32);
        for (uint256 i = 0; i < hashes.length; i++) {
            bytes32 h = hashes[i];
            assembly {
                mstore(add(add(combined, 32), mul(i, 32)), h)
            }
        }
        return sha256(combined);
    }
}
