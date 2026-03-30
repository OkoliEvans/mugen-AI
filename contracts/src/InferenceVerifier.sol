// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";
import {
    ReentrancyGuard
} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @notice Minimal interface for the EZKL-generated Halo2 circuit verifier.
interface IHalo2Verifier {
    function verifyProof(
        bytes calldata proof,
        uint256[] calldata instances
    ) external returns (bool);
}

/// @title  InferenceVerifier
/// @notice Registry and attestation layer wrapping the EZKL Halo2 circuit verifier.
///
///         Lifecycle:
///           1. Owner deploys with the Halo2Verifier address.
///           2. Owner whitelists one or more settler addresses (the Rust auto-settler).
///           3. Owner or settler registers a model via registerModel() before jobs run.
///           4. Settler calls submitProof() after each successful off-chain proof.
///           5. Any contract or user calls isVerified() to check an output hash.
///           6. Any caller uses getModel() to verify model metadata on-chain.
///
/// @dev    Security properties:
///           - Only whitelisted settlers may submit proofs (permissioned write path).
///           - Only owner may register models (prevents unauthorised model injection).
///           - Owner is Ownable2Step — two-transaction ownership transfer prevents accidents.
///           - Contract is Pausable — owner can halt submissions without destroying state.
///           - Verified output hashes are permanent and immutable (no delete path).
///           - Registered models are permanent and immutable (no update or delete path).
///           - ReentrancyGuard on submitProof as a defense-in-depth measure.
contract InferenceVerifier is Ownable2Step, Pausable, ReentrancyGuard {
    // -------------------------------------------------------------------------
    // Types
    // -------------------------------------------------------------------------

    struct Attestation {
        bytes32 modelId; // keccak256 of the ONNX model content hash
        bytes32 inputHash; // keccak256 of the raw input (not stored on-chain)
        address settler; // which settler submitted this proof
        uint48 timestamp; // block.timestamp at submission (uint48 = fine until year 281474)
    }

    /// @notice On-chain model registration record.
    /// @dev    modelId (the mapping key) is keccak256(name, version) — computed
    ///         off-chain by the gateway and passed in as the registration key.
    ///         ipfsCidHash is keccak256 of the IPFS CID string so it fits in bytes32.
    ///         The full CID string is emitted in ModelRegistered for off-chain indexing.
    struct Model {
        bytes32 ipfsCidHash;    // keccak256 of the IPFS CID string
        bytes32 inputShapeHash; // keccak256 of the ABI-encoded input shape
        address registeredBy;   // owner address that registered this model
        uint48  registeredAt;   // block.timestamp at registration
    }

    // -------------------------------------------------------------------------
    // Storage
    // -------------------------------------------------------------------------

    /// @notice The immutable EZKL-generated circuit verifier.
    /// @dev    Can be updated by owner via upgradeVerifier() for new model versions.
    IHalo2Verifier public halo2Verifier;

    /// @notice Settler whitelist. Only settlers may call submitProof().
    mapping(address => bool) public isSettler;

    /// @notice outputHash → attestation record. Permanent once written.
    mapping(bytes32 => Attestation) private _attestations;

    /// @notice outputHash → verified flag. Separate for cheap O(1) lookups.
    mapping(bytes32 => bool) public isVerified;

    /// @notice modelId → model registration record. Permanent once written.
    mapping(bytes32 => Model) private _models;

    /// @notice modelId → registered flag. Separate for cheap O(1) lookups.
    mapping(bytes32 => bool) public isRegisteredModel;

    // -------------------------------------------------------------------------
    // Events
    // -------------------------------------------------------------------------

    event InferenceVerified(
        bytes32 indexed outputHash,
        bytes32 indexed modelId,
        address indexed settler,
        uint256 timestamp
    );

    /// @notice Emitted when a model is registered on-chain.
    /// @param modelId        keccak256(name, version) — the model's permanent identifier.
    /// @param ipfsCid        Full IPFS CID string (index off-chain via this event).
    /// @param ipfsCidHash    keccak256 of ipfsCid — stored in the Model struct.
    /// @param inputShapeHash keccak256 of the ABI-encoded input shape array.
    /// @param registeredBy   Owner address that submitted the registration.
    event ModelRegistered(
        bytes32 indexed modelId,
        string          ipfsCid,
        bytes32 indexed ipfsCidHash,
        bytes32         inputShapeHash,
        address indexed registeredBy
    );

    event SettlerUpdated(address indexed settler, bool approved);
    event VerifierUpgraded(
        address indexed oldVerifier,
        address indexed newVerifier
    );

    // -------------------------------------------------------------------------
    // Errors
    // -------------------------------------------------------------------------

    error NotSettler(address caller);
    error AlreadyVerified(bytes32 outputHash);
    error InvalidProof();
    error ZeroAddress();
    error ModelAlreadyRegistered(bytes32 modelId);
    error ModelNotRegistered(bytes32 modelId);
    error EmptyString();

    // -------------------------------------------------------------------------
    // Constructor
    // -------------------------------------------------------------------------

    /// @param _halo2Verifier Address of the deployed EZKL Halo2Verifier contract.
    /// @param _initialOwner  Address that will own this contract (use a multisig in prod).
    constructor(
        address _halo2Verifier,
        address _initialOwner
    ) Ownable(_initialOwner) Pausable() ReentrancyGuard() {
        if (_halo2Verifier == address(0)) revert ZeroAddress();

        halo2Verifier = IHalo2Verifier(_halo2Verifier);
    }

    // -------------------------------------------------------------------------
    // Core — Model Registration
    // -------------------------------------------------------------------------

    /// @notice Register a model on-chain after it has been pinned to IPFS.
    ///         Must be called by the owner before any jobs can be settled for
    ///         this model. Registration is permanent — no update or delete path.
    ///
    /// @param modelId        keccak256(abi.encodePacked(name, version)).
    ///                       Computed off-chain by the gateway; must be globally unique.
    /// @param ipfsCid        Full IPFS CID string of the model artifact (e.g. "Qm...").
    ///                       Emitted in ModelRegistered for off-chain indexers.
    /// @param inputShapeHash keccak256 of the ABI-encoded input shape array.
    ///                       Lets callers verify input compatibility without storing
    ///                       the full shape on-chain.
    ///
    /// @dev  Reverts if modelId has already been registered or if ipfsCid is empty.
    function registerModel(
        bytes32 modelId,
        string calldata ipfsCid,
        bytes32 inputShapeHash
    ) external onlyOwner {
        if (isRegisteredModel[modelId]) revert ModelAlreadyRegistered(modelId);
        if (bytes(ipfsCid).length == 0) revert EmptyString();

        bytes32 ipfsCidHash = keccak256(bytes(ipfsCid));

        _models[modelId] = Model({
            ipfsCidHash:    ipfsCidHash,
            inputShapeHash: inputShapeHash,
            registeredBy:   msg.sender,
            registeredAt:   uint48(block.timestamp)
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
    // Core — Proof Submission
    // -------------------------------------------------------------------------

    /// @notice Submit a ZK proof for an inference result.
    ///         Reverts if the proof is invalid or the output has already been attested.
    ///
    /// @param proof       Raw proof bytes from the EZKL prover.
    /// @param instances   Public inputs/outputs of the circuit (model outputs as field elements).
    /// @param modelId     keccak256 of the ONNX model content hash registered off-chain.
    /// @param inputHash   keccak256 of the raw inference input (privacy-preserving).
    /// @param outputHash  keccak256 of the claimed inference output. Stored as the attestation key.
    function submitProof(
        bytes calldata proof,
        uint256[] calldata instances,
        bytes32 modelId,
        bytes32 inputHash,
        bytes32 outputHash
    ) external nonReentrant whenNotPaused {
        if (!isSettler[msg.sender]) revert NotSettler(msg.sender);
        if (isVerified[outputHash]) revert AlreadyVerified(outputHash);

        // Delegate proof verification to the circuit verifier.
        // This is the BN254 pairing check — the cryptographic core.
        if (!halo2Verifier.verifyProof(proof, instances)) revert InvalidProof();

        // Write attestation — permanent, no update or delete path.
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
    // Views
    // -------------------------------------------------------------------------

    /// @notice Returns the full attestation record for a verified output hash.
    /// @dev    Returns zero-value struct if outputHash has not been verified.
    function getAttestation(
        bytes32 outputHash
    ) external view returns (Attestation memory) {
        return _attestations[outputHash];
    }

    /// @notice Returns the registration record for a model.
    /// @dev    Reverts if the model has not been registered.
    function getModel(
        bytes32 modelId
    ) external view returns (Model memory) {
        if (!isRegisteredModel[modelId]) revert ModelNotRegistered(modelId);
        return _models[modelId];
    }

    /// @notice Derive the modelId key used throughout this contract.
    /// @dev    Convenience helper — callers can compute this off-chain identically via
    ///         keccak256(abi.encodePacked(name, version)).
    function computeModelId(
        string calldata name,
        string calldata version
    ) external pure returns (bytes32) {
        return keccak256(abi.encodePacked(name, version));
    }

    // -------------------------------------------------------------------------
    // Admin — Settler Management
    // -------------------------------------------------------------------------

    /// @notice Approve or revoke a settler address.
    /// @dev    Emits SettlerUpdated. Use a multisig as owner in production.
    function setSettler(address settler, bool approved) external onlyOwner {
        if (settler == address(0)) revert ZeroAddress();
        isSettler[settler] = approved;
        emit SettlerUpdated(settler, approved);
    }

    // -------------------------------------------------------------------------
    // Admin — Verifier Upgrade
    // -------------------------------------------------------------------------

    /// @notice Replace the underlying Halo2 circuit verifier.
    ///         Required when a new model version produces a new circuit/vk.
    /// @dev    Does NOT invalidate existing attestations — they were valid under
    ///         the old verifier and remain so. New submissions use the new verifier.
    function upgradeVerifier(address newVerifier) external onlyOwner {
        if (newVerifier == address(0)) revert ZeroAddress();
        address old = address(halo2Verifier);
        halo2Verifier = IHalo2Verifier(newVerifier);
        emit VerifierUpgraded(old, newVerifier);
    }

    // -------------------------------------------------------------------------
    // Admin — Circuit Breaker
    // -------------------------------------------------------------------------

    /// @notice Pause proof submissions. Existing attestations remain readable.
    function pause() external onlyOwner {
        _pause();
    }

    /// @notice Resume proof submissions.
    function unpause() external onlyOwner {
        _unpause();
    }
}
