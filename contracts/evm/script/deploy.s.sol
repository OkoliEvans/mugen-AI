// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console2} from "forge-std/Script.sol";
import {InferenceVerifier}  from "../src/InferenceVerifier.sol";
import {InferenceBridge}    from "../src/InferenceBridge.sol";
import {AggregatedVerifier} from "../src/AggregatedVerifier.sol";

/// @notice Deploys Mugen contracts to Eth Sepolia and optionally registers
///         an initial model on-chain.
///
/// Usage — deploy InferenceVerifier + InferenceBridge (StarkNet path):
///   forge script script/Deploy.s.sol:Deploy \
///     --sig "deployInference()" \
///     --rpc-url $RPC_URL \
///     --private-key $PRIVATE_KEY \
///     --broadcast \
///     --verify \
///     --etherscan-api-key $ETHERSCAN_API_KEY
///
/// Usage — deploy both verifiers:
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
/// Usage — deploy InferenceBridge only:
///   forge script script/Deploy.s.sol:Deploy \
///     --sig "deployBridge()" \
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
/// Required for full deploy (run) and deployInference():
///   HALO2_VERIFIER_ADDRESS     — already-deployed single-proof Halo2Verifier on Eth Sepolia
///
/// Required for deployInference() and deployBridge() (StarkNet path):
///   STARKNET_CORE_ADDRESS      — StarkNet messaging contract on Eth Sepolia
///                                (0xde29d060D45901Fb19ED6C6e959EB22d8626708e)
///   CAIRO_DEST                 — InferenceVerifier.cairo address as uint256 (felt252)
///   CAIRO_SELECTOR             — selector for consume_inference_result
///                                Compute: python3 -c "from starknet_py.hash.selector import get_selector_from_name; print(get_selector_from_name('consume_inference_result'))"
///
/// Required for aggregated deploy (deployAggregated):
///   HALO2_AGG_VERIFIER_ADDRESS — aggregation circuit verifier
///                                (deploy separately with ezkl create-evm-verifier-aggr)
///
/// Optional — initial model registration (run + deployInference + registerModel):
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
        try vm.envString("MODEL_NAME")       returns (string memory v) { if (bytes(v).length == 0) return false; } catch { return false; }
        try vm.envString("MODEL_VERSION")    returns (string memory v) { if (bytes(v).length == 0) return false; } catch { return false; }
        try vm.envString("MODEL_IPFS_CID")   returns (string memory v) { if (bytes(v).length == 0) return false; } catch { return false; }
        try vm.envBytes("MODEL_INPUT_SHAPE") returns (bytes memory v)  { if (v.length == 0) return false; }        catch { return false; }
        return true;
    }

    /// @dev Registers a model on iv. Caller must already be broadcasting.
    function _registerModel(InferenceVerifier iv) internal {
        string memory name       = vm.envString("MODEL_NAME");
        string memory version    = vm.envString("MODEL_VERSION");
        string memory ipfsCid    = vm.envString("MODEL_IPFS_CID");
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

    // ── Deploy both verifiers ─────────────────────────────────────────────────

    function run() external {
        address halo2Verifier    = vm.envAddress("HALO2_VERIFIER_ADDRESS");
        address halo2AggVerifier = vm.envAddress("HALO2_AGG_VERIFIER_ADDRESS");
        address owner            = vm.envAddress("OWNER_ADDRESS");
        address settler          = vm.envAddress("SETTLER_ADDRESS");

        console2.log("=== Deploying InferenceVerifier (Eth Sepolia) ===");
        console2.log("  Halo2Verifier    :", halo2Verifier);
        console2.log("  Owner            :", owner);
        console2.log("  Settler          :", settler);

        console2.log("=== Deploying AggregatedVerifier (Eth Sepolia) ===");
        console2.log("  Halo2AggVerifier :", halo2AggVerifier);
        console2.log("  Owner            :", owner);
        console2.log("  Settler          :", settler);

        vm.startBroadcast();

        InferenceVerifier iv = new InferenceVerifier(halo2Verifier, owner);
        iv.setSettler(settler, true);

        AggregatedVerifier av = new AggregatedVerifier(halo2AggVerifier, owner);
        av.setSettler(settler, true);

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

    // ── Deploy InferenceVerifier + InferenceBridge ────────────────────────────

    /// @notice Deploys InferenceVerifier and InferenceBridge on Eth Sepolia,
    ///         whitelists the settler, and optionally registers an initial model.
    ///
    ///         InferenceBridge is required for StarkNet settlement — it runs
    ///         the KZG pairing check on L1 then relays the result to
    ///         InferenceVerifier.cairo via IStarknetMessaging.sendMessageToL2().
    ///
    ///         Required env vars:
    ///           HALO2_VERIFIER_ADDRESS, OWNER_ADDRESS, SETTLER_ADDRESS,
    ///           STARKNET_CORE_ADDRESS, CAIRO_DEST, CAIRO_SELECTOR
    ///         Optional:
    ///           MODEL_NAME, MODEL_VERSION, MODEL_IPFS_CID, MODEL_INPUT_SHAPE
    function deployInference() external {
        address halo2Verifier  = vm.envAddress("HALO2_VERIFIER_ADDRESS");
        address owner          = vm.envAddress("OWNER_ADDRESS");
        address settler        = vm.envAddress("SETTLER_ADDRESS");
        address starknetCore   = vm.envAddress("STARKNET_CORE_ADDRESS");
        uint256 cairoDest      = vm.envUint("CAIRO_DEST");
        uint256 cairoSelector  = vm.envUint("CAIRO_SELECTOR");

        console2.log("=== Deploying InferenceVerifier (Eth Sepolia) ===");
        console2.log("  Halo2Verifier  :", halo2Verifier);
        console2.log("  Owner          :", owner);
        console2.log("  Settler        :", settler);

        console2.log("=== Deploying InferenceBridge (StarkNet path) ===");
        console2.log("  StarknetCore   :", starknetCore);
        console2.log("  CairoDest      :", cairoDest);
        console2.log("  CairoSelector  :", cairoSelector);

        vm.startBroadcast();

        InferenceVerifier iv = new InferenceVerifier(halo2Verifier, owner);
        iv.setSettler(settler, true);

        InferenceBridge bridge = new InferenceBridge(
            halo2Verifier,
            starknetCore,
            cairoDest,
            cairoSelector
        );

        if (_hasModelVars()) {
            _registerModel(iv);
        } else {
            console2.log("  MODEL_* env vars not set -- skipping model registration");
            console2.log("  Run registerModel() separately once IPFS CID is available");
        }

        vm.stopBroadcast();

        console2.log("InferenceVerifier deployed at :", address(iv));
        console2.log("InferenceBridge deployed at   :", address(bridge));
        console2.log("Settler whitelisted           :", settler);
        console2.log("");
        console2.log("Post-deploy -- add to .env:");
        console2.log("  INFERENCE_VERIFIER_ADDRESS   =", vm.toString(address(iv)));
        console2.log("  ETH_SEPOLIA_INFERENCE_BRIDGE =", vm.toString(address(bridge)));
        console2.log("");
        console2.log("Then whitelist the bridge on InferenceVerifier.cairo:");
        console2.log("  add_l1_verifier(", vm.toString(address(bridge)), ")");
    }

    // ── Deploy InferenceBridge only ───────────────────────────────────────────

    /// @notice Deploys only InferenceBridge on Eth Sepolia.
    ///         Use when InferenceVerifier is already deployed and you only
    ///         need to (re)deploy the StarkNet bridge.
    ///
    ///         Required env vars:
    ///           HALO2_VERIFIER_ADDRESS, STARKNET_CORE_ADDRESS,
    ///           CAIRO_DEST, CAIRO_SELECTOR
    function deployBridge() external {
        address halo2Verifier = vm.envAddress("HALO2_VERIFIER_ADDRESS");
        address starknetCore  = vm.envAddress("STARKNET_CORE_ADDRESS");
        uint256 cairoDest     = vm.envUint("CAIRO_DEST");
        uint256 cairoSelector = vm.envUint("CAIRO_SELECTOR");

        console2.log("=== Deploying InferenceBridge only (Eth Sepolia) ===");
        console2.log("  Halo2Verifier  :", halo2Verifier);
        console2.log("  StarknetCore   :", starknetCore);
        console2.log("  CairoDest      :", cairoDest);
        console2.log("  CairoSelector  :", cairoSelector);

        vm.startBroadcast();

        InferenceBridge bridge = new InferenceBridge(
            halo2Verifier,
            starknetCore,
            cairoDest,
            cairoSelector
        );

        vm.stopBroadcast();

        console2.log("InferenceBridge deployed at:", address(bridge));
        console2.log("");
        console2.log("Post-deploy -- add to .env:");
        console2.log("  ETH_SEPOLIA_INFERENCE_BRIDGE =", vm.toString(address(bridge)));
        console2.log("");
        console2.log("Then whitelist the bridge on InferenceVerifier.cairo:");
        console2.log("  add_l1_verifier(", vm.toString(address(bridge)), ")");
    }

    // ── Deploy AggregatedVerifier only ────────────────────────────────────────

    function deployAggregated() external {
        address halo2AggVerifier = vm.envAddress("HALO2_AGG_VERIFIER_ADDRESS");
        address owner            = vm.envAddress("OWNER_ADDRESS");
        address settler          = vm.envAddress("SETTLER_ADDRESS");

        console2.log("=== Deploying AggregatedVerifier only (Eth Sepolia) ===");
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
