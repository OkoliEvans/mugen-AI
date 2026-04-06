// script/AddGroth16Route.s.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";
import {SP1VerifierGateway} from "../lib/sp1-contracts/contracts/src/SP1VerifierGateway.sol";
import {SP1Verifier as SP1VerifierGroth16} from "../lib/sp1-contracts/contracts/src/v6.0.0/SP1VerifierGroth16.sol";

contract AddGroth16Route is Script {
    function run() external {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");

        vm.startBroadcast(deployerKey);

        SP1VerifierGroth16 groth16Verifier = new SP1VerifierGroth16();
        console.log("SP1VerifierGroth16 :", address(groth16Verifier));

        SP1VerifierGateway gateway = SP1VerifierGateway(
            0x0Be1C31a27F6477dd5DeB4eC4302B4cF199362CF
        );
        gateway.addRoute(address(groth16Verifier));
        console.log("Groth16 route registered");

        bytes4 selector = bytes4(groth16Verifier.VERIFIER_HASH());
        console.logBytes4(selector);

        vm.stopBroadcast();
    }
}