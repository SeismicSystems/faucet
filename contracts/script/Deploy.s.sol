// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.0;

import "forge-std/Script.sol";
import "../src/SeismicFaucet.sol";

contract DeployScript is Script {
    function run() external {
        // Faucet account
        uint256 faucetPrivateKey = vm.envUint("FAUCET_PRIVATE_KEY");

        // Reserve account
        uint256 reservePrivateKey = vm.envUint("FAUCET_RESERVE_PRIVATE_KEY");
        address reserveAccount = vm.addr(reservePrivateKey);
        address faucetAccount = vm.addr(faucetPrivateKey);

        vm.startBroadcast(faucetPrivateKey);

        SeismicFaucet faucet = new SeismicFaucet();

        faucet.updateSuperOperator(reserveAccount, true);

        faucet.updateApprovedOperator(faucetAccount, true);

        payable(address(faucet)).transfer(2000 ether);

        vm.stopBroadcast();

        console.log("=== SeismicFaucet Deployment ===");
        console.log("Contract deployed to:", address(faucet));
        console.log("Chain ID:", block.chainid);
        console.log("ETH drip amount:", faucet.ETH_AMOUNT());
        console.log("Initial funding:", 2000, "ETH");
        console.log("Available drips:", faucet.availableDrips());
        console.log("Deployer/Faucet (super + approved operator):", faucetAccount);
        console.log("Reserve account (super operator):", reserveAccount);
    }
}
