// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {ISP1Verifier} from "@sp1-contracts/ISP1Verifier.sol";

/// @title  InferenceVerifier
/// @notice Registry and attestation layer wrapping the SP1 universal verifier.
///
///         Lifecycle:
///           1. Owner deploys with the SP1VerifierGateway address and inference vkey.
///           2. Owner whitelists one or more settler addresses.
///           3. Owner or settler registers a model via registerModel() before jobs run.
///           4. Settler calls submitProof() after each successful off-chain SP1 proof.
///           5. Any contract or user calls isVerified() to check an output hash.
///           6. Any caller uses getModel() to verify model metadata on-chain.
///
/// @dev    Key difference from EZKL version:
///           - Uses SP1VerifierGateway (universal) instead of a circuit-specific Halo2Verifier.
///           - One verifier contract handles all SP1 programs — no redeploy per model.
///           - Proof verification delegates to ISP1Verifier.verifyProof(vkey, publicValues, proof).
///           - publicValues are ABI-encoded (modelId, inputHash, outputHash) committed by guest.
contract InferenceVerifier is Ownable2Step, Pausable, ReentrancyGuard {

    // -------------------------------------------------------------------------
    // Types
    // -------------------------------------------------------------------------

    struct Attestation {
        bytes32 modelId;      // sha256 of model weights — committed by SP1 guest
        bytes32 inputHash;    // sha256 of raw input — committed by SP1 guest
        address settler;      // which settler submitted this proof
        uint48  timestamp;    // block.timestamp at submission
    }

    struct Model {
        bytes32 ipfsCidHash;    // keccak256 of the IPFS CID string
        bytes32 inputShapeHash; // keccak256 of ABI-encoded input shape
        address registeredBy;
        uint48  registeredAt;
    }

    // -------------------------------------------------------------------------
    // Storage
    // -------------------------------------------------------------------------

    /// @notice SP1VerifierGateway — universal verifier deployed by Succinct.
    ///         Routes proofs to the correct versioned verifier automatically.
    ISP1Verifier public sp1Verifier;

    /// @notice The SP1 program verification key for the inference guest program.
    ///         Computed via: cargo prove vkey --elf inference-guest
    ///         Uniquely identifies our specific guest program — prevents proof
    ///         from a different SP1 program being accepted.
    bytes32 public inferenceVKey;

    /// @notice Settler whitelist.
    mapping(address => bool) public isSettler;

    /// @notice outputHash → attestation. Permanent once written.
    mapping(bytes32 => Attestation) private _attestations;

    /// @notice outputHash → verified flag. Separate for cheap O(1) lookups.
    mapping(bytes32 => bool) public isVerified;

    /// @notice modelId → model registration record.
    mapping(bytes32 => Model) private _models;

    /// @notice modelId → registered flag.
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

    event VKeyUpdated(
        bytes32 indexed oldVKey,
        bytes32 indexed newVKey
    );

    // -------------------------------------------------------------------------
    // Errors
    // -------------------------------------------------------------------------

    error NotSettler(address caller);
    error AlreadyVerified(bytes32 outputHash);
    error InvalidProof();
    error ZeroAddress();
    error ZeroVKey();
    error ModelAlreadyRegistered(bytes32 modelId);
    error ModelNotRegistered(bytes32 modelId);
    error EmptyString();

    // -------------------------------------------------------------------------
    // Constructor
    // -------------------------------------------------------------------------

    /// @param _sp1Verifier   Address of the SP1VerifierGateway.
    ///                       On HashKey testnet: deploy SP1VerifierGateway yourself
    ///                       (Succinct has not deployed it on this chain yet).
    /// @param _inferenceVKey The SP1 vkey for the inference guest program.
    ///                       Get via: cargo prove vkey --elf crates/guest/elf/inference-guest
    /// @param _initialOwner  Address that will own this contract.
    constructor(
        address _sp1Verifier,
        bytes32 _inferenceVKey,
        address _initialOwner
    ) Ownable(_initialOwner) {
        if (_sp1Verifier == address(0))  revert ZeroAddress();
        if (_inferenceVKey == bytes32(0)) revert ZeroVKey();

        sp1Verifier    = ISP1Verifier(_sp1Verifier);
        inferenceVKey  = _inferenceVKey;
    }

    // -------------------------------------------------------------------------
    // Core — Model Registration
    // -------------------------------------------------------------------------

    /// @notice Register a model on-chain after it has been pinned to IPFS.
    /// @param modelId        sha256(weights_bytes) — computed by the Rust gateway.
    /// @param ipfsCid        Full IPFS CID string of the model artifact.
    /// @param inputShapeHash keccak256 of ABI-encoded input shape array.
    function registerModel(
        bytes32 modelId,
        string calldata ipfsCid,
        bytes32 inputShapeHash
    ) external onlyOwner {
        if (isRegisteredModel[modelId])   revert ModelAlreadyRegistered(modelId);
        if (bytes(ipfsCid).length == 0)   revert EmptyString();

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

    /// @notice Submit an SP1 proof of correct inference execution.
    ///
    /// @param proofBytes    Compressed SP1 STARK proof bytes.
    /// @param publicValues  ABI-encoded public values committed by the guest program:
    ///                      abi.encode(bytes32 modelId, bytes32 inputHash, bytes32 outputHash)
    ///                      These are committed via sp1_zkvm::io::commit() in the guest.
    ///
    /// @dev  The SP1VerifierGateway reads the first 4 bytes of proofBytes as a
    ///       version selector and routes to the correct versioned verifier automatically.
    ///       No per-version logic needed here.
    function submitProof(
        bytes calldata proofBytes,
        bytes calldata publicValues
    ) external nonReentrant whenNotPaused {
        if (!isSettler[msg.sender]) revert NotSettler(msg.sender);

        // Decode public values committed by the SP1 guest program.
        // Must match the order of sp1_zkvm::io::commit() calls in guest/src/main.rs:
        //   commit(&model_id)    → bytes32
        //   commit(&input_hash)  → bytes32
        //   commit(&output_hash) → bytes32
        (
            bytes32 modelId,
            bytes32 inputHash,
            bytes32 outputHash
        ) = abi.decode(publicValues, (bytes32, bytes32, bytes32));

        if (isVerified[outputHash]) revert AlreadyVerified(outputHash);

        // Delegate to SP1VerifierGateway — reverts if proof is invalid.
        // This is the cryptographic core: verifies the STARK proof against
        // our specific guest program vkey and the committed public values.
        sp1Verifier.verifyProof(inferenceVKey, publicValues, proofBytes);

        // Write attestation — permanent, no update or delete path.
        _attestations[outputHash] = Attestation({
            modelId:   modelId,
            inputHash: inputHash,
            settler:   msg.sender,
            timestamp: uint48(block.timestamp)
        });
        isVerified[outputHash] = true;

        emit InferenceVerified(outputHash, modelId, msg.sender, block.timestamp);
    }

    // -------------------------------------------------------------------------
    // Views
    // -------------------------------------------------------------------------

    function getAttestation(bytes32 outputHash)
        external view returns (Attestation memory)
    {
        return _attestations[outputHash];
    }

    function getModel(bytes32 modelId)
        external view returns (Model memory)
    {
        if (!isRegisteredModel[modelId]) revert ModelNotRegistered(modelId);
        return _models[modelId];
    }

    /// @notice Convenience: derive modelId from name + version.
    ///         Note: SP1 system uses sha256(weights_bytes) as modelId,
    ///         not keccak256(name+version). This helper is for registry
    ///         lookups only.
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

    /// @notice Upgrade the SP1VerifierGateway address.
    ///         Required if Succinct deploys an official gateway on this chain later.
    function upgradeVerifier(address newVerifier) external onlyOwner {
        if (newVerifier == address(0)) revert ZeroAddress();
        address old = address(sp1Verifier);
        sp1Verifier = ISP1Verifier(newVerifier);
        emit VerifierUpgraded(old, newVerifier);
    }

    /// @notice Update the inference guest program vkey.
    ///         Required when the guest program is updated (new model, new circuit).
    function updateVKey(bytes32 newVKey) external onlyOwner {
        if (newVKey == bytes32(0)) revert ZeroVKey();
        bytes32 old = inferenceVKey;
        inferenceVKey = newVKey;
        emit VKeyUpdated(old, newVKey);
    }

    function pause()   external onlyOwner { _pause(); }
    function unpause() external onlyOwner { _unpause(); }
}