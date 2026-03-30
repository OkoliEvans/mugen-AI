// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console2} from "forge-std/Script.sol";
import {InferenceVerifier}   from "../src/InferenceVerifier.sol";
import {AggregatedVerifier}  from "../src/AggregatedVerifier.sol";

/// @notice Deploys InferenceVerifier and AggregatedVerifier, whitelists settler,
///         and optionally registers an initial model on-chain.
///
/// Usage — deploy InferenceVerifier only (Base Sepolia / any network):
///   forge script script/Deploy.s.sol:Deploy \
///     --sig "deployInference()" \
///     --rpc-url $RPC_URL \
///     --private-key $PRIVATE_KEY \
///     --broadcast \
///     --verify \
///     --etherscan-api-key $ETHERSCAN_API_KEY
///
/// Usage — deploy both:
///   forge script script/Deploy.s.sol:Deploy \
///     --sig "run()" \
///     --rpc-url $RPC_URL \
///     --private-key $PRIVATE_KEY \
///     --broadcast \
///     --verify \
///     --etherscan-api-key $ETHERSCAN_API_KEY
///
/// Usage — deploy AggregatedVerifier only:
///   forge script script/Deploy.s.sol:Deploy \
///     --sig "deployAggregated()" \
///     --rpc-url $RPC_URL \
///     --private-key $PRIVATE_KEY \
///     --broadcast \
///     --verify \
///     --etherscan-api-key $ETHERSCAN_API_KEY
///
/// Usage — register a model on an already-deployed InferenceVerifier:
///   forge script script/Deploy.s.sol:Deploy \
///     --sig "registerModel()" \
///     --rpc-url $RPC_URL \
///     --private-key $PRIVATE_KEY \
///     --broadcast
///
/// Required env vars (all deployments):
///   OWNER_ADDRESS              — multisig or EOA that will own the contracts
///   SETTLER_ADDRESS            — the Rust auto-settler wallet address
///
/// Required for full deploy (run):
///   HALO2_VERIFIER_ADDRESS     — already-deployed single-proof Halo2Verifier
///
/// Required for aggregated deploy (deployAggregated):
///   HALO2_AGG_VERIFIER_ADDRESS — aggregation circuit verifier
///                                (deploy separately with ezkl create-evm-verifier-aggr)
///
/// Optional — initial model registration (run + registerModel):
///   MODEL_NAME                 — human-readable model name  (e.g. "tiny_mlp_v1")
///   MODEL_VERSION              — semver string              (e.g. "0.1.0")
///   MODEL_IPFS_CID             — IPFS CID of the model artifact (e.g. "QmXyz...")
///   MODEL_INPUT_SHAPE          — ABI-encoded input shape bytes (hex, no 0x prefix)
///                                Generate with: cast abi-encode "f(uint256[])" "[1,4]"
///
///   If any of the four MODEL_* vars are absent or empty the registration step
///   is skipped silently — safe to omit during initial infra setup.
///
/// Required for standalone registerModel():
///   INFERENCE_VERIFIER_ADDRESS — already-deployed InferenceVerifier
///   MODEL_NAME, MODEL_VERSION, MODEL_IPFS_CID, MODEL_INPUT_SHAPE (all required)
contract Deploy is Script {

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// @dev Returns true only when all four MODEL_* env vars are set and non-empty.
    function _hasModelVars() internal view returns (bool) {
        try vm.envString("MODEL_NAME")         returns (string memory v) { if (bytes(v).length == 0) return false; } catch { return false; }
        try vm.envString("MODEL_VERSION")      returns (string memory v) { if (bytes(v).length == 0) return false; } catch { return false; }
        try vm.envString("MODEL_IPFS_CID")     returns (string memory v) { if (bytes(v).length == 0) return false; } catch { return false; }
        try vm.envBytes("MODEL_INPUT_SHAPE")   returns (bytes memory v)  { if (v.length == 0) return false; }        catch { return false; }
        return true;
    }

    /// @dev Registers a model on iv. Caller must already be broadcasting.
    function _registerModel(InferenceVerifier iv) internal {
        string memory name       = vm.envString("MODEL_NAME");
        string memory version    = vm.envString("MODEL_VERSION");
        string memory ipfsCid   = vm.envString("MODEL_IPFS_CID");
        bytes  memory shapeBytes = vm.envBytes("MODEL_INPUT_SHAPE");

        bytes32 modelId        = keccak256(abi.encodePacked(name, version));
        bytes32 inputShapeHash = keccak256(shapeBytes);

        console2.log("  Registering model on-chain:");
        console2.log("    Name            :", name);
        console2.log("    Version         :", version);
        console2.log("    IPFS CID        :", ipfsCid);
        console2.log("    inputShapeHash  :", vm.toString(inputShapeHash));
        console2.log("    modelId         :", vm.toString(modelId));

        iv.registerModel(modelId, ipfsCid, inputShapeHash);

        console2.log("  Model registered  :", vm.toString(modelId));
    }

    // ── Deploy both ───────────────────────────────────────────────────────────

    function run() external {
        address halo2Verifier    = vm.envAddress("HALO2_VERIFIER_ADDRESS");
        address halo2AggVerifier = vm.envAddress("HALO2_AGG_VERIFIER_ADDRESS");
        address owner            = vm.envAddress("OWNER_ADDRESS");
        address settler          = vm.envAddress("SETTLER_ADDRESS");

        console2.log("=== Deploying InferenceVerifier ===");
        console2.log("  Halo2Verifier    :", halo2Verifier);
        console2.log("  Owner            :", owner);
        console2.log("  Settler          :", settler);

        console2.log("=== Deploying AggregatedVerifier ===");
        console2.log("  Halo2AggVerifier :", halo2AggVerifier);
        console2.log("  Owner            :", owner);
        console2.log("  Settler          :", settler);

        vm.startBroadcast();

        // Single-proof verifier (Phase 1)
        InferenceVerifier iv = new InferenceVerifier(halo2Verifier, owner);
        iv.setSettler(settler, true);

        // Aggregated verifier (Phase 2)
        AggregatedVerifier av = new AggregatedVerifier(halo2AggVerifier, owner);
        av.setSettler(settler, true);

        // Register initial model if MODEL_* vars are present
        if (_hasModelVars()) {
            _registerModel(iv);
        } else {
            console2.log("  MODEL_* env vars not set -- skipping model registration");
            console2.log("  Run registerModel() separately once IPFS CID is available");
        }

        vm.stopBroadcast();

        console2.log("InferenceVerifier deployed at :", address(iv));
        console2.log("AggregatedVerifier deployed at:", address(av));
        console2.log("Settler whitelisted           :", settler);
    }

    // ── Deploy InferenceVerifier only ─────────────────────────────────────────

    /// @notice Deploys only InferenceVerifier, whitelists the settler, and
    ///         optionally registers an initial model if MODEL_* vars are set.
    ///         Use this for Phase 1 deployments before aggregation is ready.
    ///
    ///         Required env vars:
    ///           HALO2_VERIFIER_ADDRESS, OWNER_ADDRESS, SETTLER_ADDRESS
    ///         Optional:
    ///           MODEL_NAME, MODEL_VERSION, MODEL_IPFS_CID, MODEL_INPUT_SHAPE
    function deployInference() external {
        address halo2Verifier = vm.envAddress("HALO2_VERIFIER_ADDRESS");
        address owner         = vm.envAddress("OWNER_ADDRESS");
        address settler       = vm.envAddress("SETTLER_ADDRESS");

        console2.log("=== Deploying InferenceVerifier (Phase 1) ===");
        console2.log("  Halo2Verifier :", halo2Verifier);
        console2.log("  Owner         :", owner);
        console2.log("  Settler       :", settler);

        vm.startBroadcast();

        InferenceVerifier iv = new InferenceVerifier(halo2Verifier, owner);
        iv.setSettler(settler, true);

        if (_hasModelVars()) {
            _registerModel(iv);
        } else {
            console2.log("  MODEL_* env vars not set -- skipping model registration");
            console2.log("  Run registerModel() separately once IPFS CID is available");
        }

        vm.stopBroadcast();

        console2.log("InferenceVerifier deployed at:", address(iv));
        console2.log("Settler whitelisted           :", settler);
        console2.log("");
        console2.log("Post-deploy -- add to .env:");
        console2.log("  INFERENCE_VERIFIER_ADDRESS =", vm.toString(address(iv)));
    }

    // ── Deploy AggregatedVerifier only ────────────────────────────────────────

    function deployAggregated() external {
        address halo2AggVerifier = vm.envAddress("HALO2_AGG_VERIFIER_ADDRESS");
        address owner            = vm.envAddress("OWNER_ADDRESS");
        address settler          = vm.envAddress("SETTLER_ADDRESS");

        console2.log("=== Deploying AggregatedVerifier only ===");
        console2.log("  Halo2AggVerifier :", halo2AggVerifier);
        console2.log("  Owner            :", owner);
        console2.log("  Settler          :", settler);

        vm.startBroadcast();

        AggregatedVerifier av = new AggregatedVerifier(halo2AggVerifier, owner);
        av.setSettler(settler, true);

        vm.stopBroadcast();

        console2.log("AggregatedVerifier deployed at:", address(av));
        console2.log("Settler whitelisted           :", settler);
    }

    // ── Register a model on an existing InferenceVerifier ────────────────────

    /// @notice Registers a model on an already-deployed InferenceVerifier.
    ///         Use this after the IPFS pin is confirmed and the full reg flow
    ///         has run on the gateway side.
    ///
    ///         Required env vars:
    ///           INFERENCE_VERIFIER_ADDRESS
    ///           MODEL_NAME, MODEL_VERSION, MODEL_IPFS_CID, MODEL_INPUT_SHAPE
    function registerModel() external {
        address ivAddress = vm.envAddress("INFERENCE_VERIFIER_ADDRESS");

        console2.log("=== Registering Model ===");
        console2.log("  InferenceVerifier:", ivAddress);

        InferenceVerifier iv = InferenceVerifier(ivAddress);

        vm.startBroadcast();
        _registerModel(iv);
        vm.stopBroadcast();
    }
}
