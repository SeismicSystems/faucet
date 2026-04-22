// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.0;

import "forge-std/Script.sol";
import "../src/SeismicFaucet.sol";
import {ISRC20} from "seismic-std-lib/interfaces/ISRC20.sol";

contract DeployScript is Script {
    /// @notice SUSDC to seed the faucet contract with (6 decimals)
    uint256 constant INITIAL_FAUCET_SEED = 1_000_000e6; // 1,000,000 SUSDC

    function run() external {
        // Faucet account (also the SUSDC admin — must pre-mint SUSDC to itself before running this script)
        uint256 faucetPrivateKey = vm.envUint("FAUCET_PRIVATE_KEY");
        uint256 reservePrivateKey = vm.envUint("FAUCET_RESERVE_PRIVATE_KEY");
        address susdcAddress = vm.envAddress("SUSDC_ADDRESS");

        address reserveAccount = vm.addr(reservePrivateKey);
        address faucetAccount = vm.addr(faucetPrivateKey);

        vm.startBroadcast(faucetPrivateKey);

        SeismicFaucet faucet = new SeismicFaucet(susdcAddress);

        faucet.updateSuperOperator(reserveAccount, true);
        faucet.updateApprovedOperator(faucetAccount, true);

        // Seed the faucet with SUSDC from the operator's balance
        require(
            ISRC20(susdcAddress).transfer(address(faucet), suint256(INITIAL_FAUCET_SEED)),
            "Failed seeding faucet with SUSDC"
        );

        vm.stopBroadcast();

        console.log("=== SeismicFaucet Deployment ===");
        console.log("Contract deployed to:", address(faucet));
        console.log("Chain ID:", block.chainid);
        console.log("SUSDC token:", susdcAddress);
        console.log("Regular drip amount (SUSDC, 6d):", faucet.USDC_AMOUNT());
        console.log("Developer drip amount (SUSDC, 6d):", faucet.DEVELOPER_USDC_AMOUNT());
        console.log("Whitelist drip amount (SUSDC, 6d):", faucet.WHITELIST_USDC_AMOUNT());
        console.log("Initial seed (SUSDC, 6d):", INITIAL_FAUCET_SEED);
        console.log("Deployer/Faucet (super + approved operator):", faucetAccount);
        console.log("Reserve account (super operator):", reserveAccount);
    }
}
