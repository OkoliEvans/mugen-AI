// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";
import {SP1VerifierGateway} from "@sp1-contracts/SP1VerifierGateway.sol";
import {
    SP1Verifier as SP1VerifierPlonk
} from "@sp1-contracts/v6.0.0/SP1VerifierPlonk.sol";
import {InferenceVerifier} from "../src/InferenceVerifier.sol";

/// @notice Deploys the full SP1 Plonk verifier stack on HashKey testnet.
///
/// Usage:
///   forge script script/Deploy.s.sol \
///     --rpc-url https://testnet.hsk.xyz \
///     --chain-id 133 \
///     --private-key $PRIVATE_KEY \
///     --broadcast \
///     -vvvv
///
/// Required env vars:
///   PRIVATE_KEY       — deployer private key (0x prefixed)
///   SETTLER_ADDRESS   — address to whitelist as settler

contract Deploy is Script {
    // Inference guest vkey — generated via:
    // cargo prove vkey --elf target/elf-compilation/riscv64im-succinct-zkvm-elf/release/inference-guest
    bytes32 constant INFERENCE_VKEY =
        0x00bb7d2c3c965a0188abfb2d674d31e0ba558c62e4e1bad5574afe00545a75fd;

    // Aggregator guest vkey — generated via:
    // cargo prove vkey --elf target/elf-compilation/riscv64im-succinct-zkvm-elf/release/aggregator-guest
    bytes32 constant AGGREGATION_VKEY =
        0x00b766856cc9b7ae52a999b3eef9dd1d16a2d8cfd61e79f6f63ba4ae172038d0;

    function run() external {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address settlerAddress = vm.envAddress("SETTLER_ADDRESS");
        address deployer = vm.addr(deployerKey);

        console.log("Deployer          :", deployer);
        console.log("Inference VKey    :", vm.toString(INFERENCE_VKEY));
        console.log("Aggregation VKey  :", vm.toString(AGGREGATION_VKEY));
        console.log("Settler           :", settlerAddress);

        vm.startBroadcast(deployerKey);

        // 1. Deploy PlonkVerifier (the actual Gnark crypto verifier)
        SP1VerifierPlonk plonkVerifier = new SP1VerifierPlonk();
        console.log("SP1VerifierPlonk   :", address(plonkVerifier));

        // 2. Deploy SP1VerifierGateway (routes proofs to versioned verifiers)
        SP1VerifierGateway gateway = new SP1VerifierGateway(deployer);
        console.log("SP1VerifierGateway :", address(gateway));

        // 3. Register the Plonk verifier route with the gateway
        gateway.addRoute(address(plonkVerifier));
        console.log("Plonk route registered");

        // 4. Deploy InferenceVerifier wrapping the gateway
        InferenceVerifier verifier = new InferenceVerifier(
            address(gateway),
            INFERENCE_VKEY,
            deployer
        );
        console.log("InferenceVerifier  :", address(verifier));

        // 5. Whitelist the settler
        verifier.setSettler(settlerAddress, true);
        console.log("Settler whitelisted:", settlerAddress);

        // 6. Register the aggregator guest vkey
        //    Required before submitAggregatedProof() can be called.
        verifier.setAggregationVKey(AGGREGATION_VKEY);
        console.log("Aggregation VKey set");

        vm.stopBroadcast();

        console.log("\n-- Deployment complete --");
        console.log("Add to .env:");
        console.log("  SP1_VERIFIER_GATEWAY=", address(gateway));
        console.log("  INFERENCE_VERIFIER=  ", address(verifier));
    }
}