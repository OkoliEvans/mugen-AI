// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Ownable2Step, Ownable} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @dev Minimal interface for the EZKL-generated Halo2 aggregation verifier.
interface IHalo2AggVerifier {
    function verifyProof(bytes calldata proof, uint256[] calldata instances)
        external
        view
        returns (bool);
}

/// @title AggregatedVerifier
/// @notice Accepts batched ZK proofs and records all attested output hashes.
///         One on-chain tx settles up to 500 individual inference proofs.
contract AggregatedVerifier is Ownable2Step, Pausable, ReentrancyGuard {

    // ── State ─────────────────────────────────────────────────────────────────

    IHalo2AggVerifier public immutable halo2Verifier;

    /// output_hash => true once verified
    mapping(bytes32 => bool) public isVerified;

    /// Whitelisted addresses that may submit batches
    mapping(address => bool) public isSettler;

    // ── Events ────────────────────────────────────────────────────────────────

    event BatchSettled(
        bytes32 indexed batchId,
        uint256          jobCount,
        address indexed  settler,
        uint256          timestamp
    );
    event OutputVerified(bytes32 indexed outputHash, bytes32 indexed batchId);
    event SettlerUpdated(address indexed settler, bool enabled);

    // ── Errors ────────────────────────────────────────────────────────────────

    error NotSettler(address caller);
    error InvalidProof();
    error AlreadyVerified(bytes32 outputHash);
    error EmptyBatch();
    error BatchTooLarge(uint256 size, uint256 max);

    // ── Constants ─────────────────────────────────────────────────────────────

    uint256 public constant MAX_BATCH_SIZE = 500;

    // ── Constructor ───────────────────────────────────────────────────────────

    constructor(address _halo2Verifier, address _initialOwner)
        Ownable(_initialOwner)
    {
        halo2Verifier = IHalo2AggVerifier(_halo2Verifier);
        isSettler[_initialOwner] = true;
    }

    // ── Settler management ────────────────────────────────────────────────────

    function setSettler(address settler, bool enabled) external onlyOwner {
        isSettler[settler] = enabled;
        emit SettlerUpdated(settler, enabled);
    }

    // ── Core: submit a batch ─────────────────────────────────────────────────

    /// @notice Submit an aggregated proof covering a batch of inference jobs.
    /// @param proof        Aggregated KZG/Halo2 proof bytes
    /// @param instances    Public instances for the aggregated circuit
    /// @param batchId      Off-chain batch UUID (as bytes32)
    /// @param outputHashes keccak256 output hash for each job in the batch
    function submitBatch(
        bytes     calldata proof,
        uint256[] calldata instances,
        bytes32            batchId,
        bytes32[] calldata outputHashes
    )
        external
        nonReentrant
        whenNotPaused
    {
        if (!isSettler[msg.sender]) revert NotSettler(msg.sender);
        if (outputHashes.length == 0) revert EmptyBatch();
        if (outputHashes.length > MAX_BATCH_SIZE)
            revert BatchTooLarge(outputHashes.length, MAX_BATCH_SIZE);

        // Verify the aggregated proof once — covers all jobs in the batch
        if (!halo2Verifier.verifyProof(proof, instances)) revert InvalidProof();

        // Record each output hash
        for (uint256 i = 0; i < outputHashes.length; i++) {
            bytes32 h = outputHashes[i];
            if (isVerified[h]) revert AlreadyVerified(h);
            isVerified[h] = true;
            emit OutputVerified(h, batchId);
        }

        emit BatchSettled(batchId, outputHashes.length, msg.sender, block.timestamp);
    }

    // ── View helpers ─────────────────────────────────────────────────────────

    /// @notice Check whether all hashes in a batch have been verified.
    function allVerified(bytes32[] calldata outputHashes)
        external
        view
        returns (bool)
    {
        for (uint256 i = 0; i < outputHashes.length; i++) {
            if (!isVerified[outputHashes[i]]) return false;
        }
        return true;
    }

    // ── Emergency ─────────────────────────────────────────────────────────────

    function pause()   external onlyOwner { _pause(); }
    function unpause() external onlyOwner { _unpause(); }
}
