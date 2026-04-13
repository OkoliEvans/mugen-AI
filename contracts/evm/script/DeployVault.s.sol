// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";
import {VeilVault}        from "../src/VeilVault.sol";

contract DeployVault is Script {
    function run() external {
        address gateway = vm.envAddress("GATEWAY_WALLET_ADDRESS");
        address owner   = vm.envAddress("OWNER_ADDRESS");

        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        vm.startBroadcast(deployerKey);

        VeilVault vault = new VeilVault(gateway, owner);

        console.log("VeilVault deployed at:", address(vault));
        console.log("Gateway:              ", gateway);
        console.log("Owner:                ", owner);
        console.log("Standard fee (HSK):   ", vault.STANDARD_FEE() / 1e18);
        console.log("Priority fee (HSK):   ", vault.PRIORITY_FEE() / 1e18);

        vm.stopBroadcast();
    }
}
