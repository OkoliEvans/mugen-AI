// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";
import {InferenceVerifier} from "../src/InferenceVerifier.sol";

/// @notice Redeploys InferenceVerifier with aggregation support.
///         Gateway and verifier routes are already deployed — reused as-is.
///
/// Usage:
///   forge script script/RedeployInferenceVerifier.s.sol \
///     --rpc-url https://testnet.hsk.xyz \
///     --chain-id 133 \
///     --private-key $PRIVATE_KEY \
///     --broadcast \
///     -vvvv
///
/// Required env vars:
///   PRIVATE_KEY              — deployer private key (0x prefixed)
///   SETTLER_ADDRESS          — address to whitelist as settler
///   SP1_VERIFIER_GATEWAY     — already deployed gateway address

contract RedeployInferenceVerifier is Script {
    bytes32 constant INFERENCE_VKEY =
        0x00bb7d2c3c965a0188abfb2d674d31e0ba558c62e4e1bad5574afe00545a75fd;

    bytes32 constant AGGREGATION_VKEY =
        0x00b766856cc9b7ae52a999b3eef9dd1d16a2d8cfd61e79f6f63ba4ae172038d0;

    function run() external {
        uint256 deployerKey    = vm.envUint("PRIVATE_KEY");
        address settlerAddress = vm.envAddress("SETTLER_ADDRESS");
        address gateway        = vm.envAddress("SP1_VERIFIER_GATEWAY");
        address deployer       = vm.addr(deployerKey);

        console.log("Deployer         :", deployer);
        console.log("Gateway (reused) :", gateway);
        console.log("Inference VKey   :", vm.toString(INFERENCE_VKEY));
        console.log("Aggregation VKey :", vm.toString(AGGREGATION_VKEY));
        console.log("Settler          :", settlerAddress);

        vm.startBroadcast(deployerKey);

        // 1. Deploy new InferenceVerifier pointing at existing gateway
        InferenceVerifier verifier = new InferenceVerifier(
            gateway,
            INFERENCE_VKEY,
            deployer
        );
        console.log("InferenceVerifier:", address(verifier));

        // 2. Whitelist the settler
        verifier.setSettler(settlerAddress, true);
        console.log("Settler whitelisted");

        // 3. Set aggregation vkey — enables submitAggregatedProof()
        verifier.setAggregationVKey(AGGREGATION_VKEY);
        console.log("Aggregation VKey set");

        vm.stopBroadcast();

        console.log("\n-- Done. Update .env:");
        console.log("  INFERENCE_VERIFIER_ADDRESS=", address(verifier));
    }
}