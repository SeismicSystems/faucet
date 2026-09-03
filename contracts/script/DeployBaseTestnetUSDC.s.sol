// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

import {Script, console} from "forge-std/Script.sol";
import {TestnetUSDC} from "../src/TestnetUSDC.sol";

/// @notice Deploys the controlled six-decimal test USDC on Base Sepolia and
/// mints the initial supply into the machine-funding reserve. There is no
/// faucet contract on Base: the reserve key transfers directly, so the
/// reserve holds the tokens and the deployer keeps mint authority.
contract DeployBaseTestnetUSDCScript is Script {
    uint256 internal constant DEFAULT_INITIAL_RESERVE_SUPPLY = 1_000_000e6;

    function run() external {
        uint256 deployerPrivateKey = vm.envUint("BASE_DEPLOYER_PRIVATE_KEY");
        address reserveAccount = vm.envAddress("INTERNAL_FUNDING_BASE_ADDRESS");
        uint256 initialSupply = vm.envOr("BASE_ERC20_USDC_INITIAL_RESERVE_SUPPLY", DEFAULT_INITIAL_RESERVE_SUPPLY);

        address deployer = vm.addr(deployerPrivateKey);
        require(reserveAccount != address(0), "Invalid reserve account");
        require(initialSupply > 0, "Invalid initial supply");
        vm.startBroadcast(deployerPrivateKey);

        TestnetUSDC usdc = new TestnetUSDC(deployer);
        usdc.mint(reserveAccount, initialSupply);

        vm.stopBroadcast();

        console.log("=== Base Testnet USDC Deployment ===");
        console.log("USDC token:", address(usdc));
        console.log("Chain ID:", block.chainid);
        console.log("Token owner / minter:", deployer);
        console.log("Machine funding reserve:", reserveAccount);
        console.log("Initial reserve supply (USDC, 6d):", initialSupply);
    }
}
