// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

import {Script, console} from "forge-std/Script.sol";
import {ERC20USDCFaucet} from "../src/ERC20USDCFaucet.sol";
import {TestnetUSDC} from "../src/TestnetUSDC.sol";

contract DeployERC20USDCScript is Script {
    uint256 internal constant INITIAL_FAUCET_SEED = 1_000_000e6;

    function run() external {
        uint256 faucetPrivateKey = vm.envUint("FAUCET_PRIVATE_KEY");
        address machineFundingAccount = vm.envAddress("INTERNAL_FUNDING_ERC20_USDC_ADDRESS");
        address reserveAccount = vm.envAddress("ERC20_USDC_RESERVE_ADDRESS");
        require(machineFundingAccount != address(0), "Invalid machine funding account");

        address faucetAccount = vm.addr(faucetPrivateKey);
        require(reserveAccount != address(0), "Invalid reserve account");
        require(reserveAccount != faucetAccount, "Reserve must differ from deployer");
        require(reserveAccount != machineFundingAccount, "Reserve must differ from machine operator");
        vm.startBroadcast(faucetPrivateKey);

        TestnetUSDC usdc = new TestnetUSDC(faucetAccount);
        ERC20USDCFaucet faucet = new ERC20USDCFaucet(address(usdc), machineFundingAccount);
        usdc.mint(address(faucet), INITIAL_FAUCET_SEED);
        usdc.transferOwnership(reserveAccount);
        faucet.updateSuperOperator(reserveAccount, true);
        faucet.updateSuperOperator(faucetAccount, false);

        vm.stopBroadcast();

        console.log("=== ERC20 USDC Faucet Deployment ===");
        console.log("USDC token:", address(usdc));
        console.log("ERC20 USDC faucet:", address(faucet));
        console.log("Chain ID:", block.chainid);
        console.log("Initial faucet seed (USDC, 6d):", INITIAL_FAUCET_SEED);
        console.log("Token owner / faucet recovery operator:", reserveAccount);
        console.log("Machine funding account:", machineFundingAccount);
    }
}
