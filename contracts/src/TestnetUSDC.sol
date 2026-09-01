// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";

/// @notice Standard six-decimal ERC20 USDC used on Seismic testnets.
contract TestnetUSDC is ERC20, Ownable {
    uint8 internal constant TOKEN_DECIMALS = 6;

    constructor(address initialOwner) ERC20("Seismic Testnet USDC", "USDC") {
        require(initialOwner != address(0), "Invalid owner");
        _transferOwnership(initialOwner);
    }

    function decimals() public pure override returns (uint8) {
        return TOKEN_DECIMALS;
    }

    function mint(address recipient, uint256 amount) external onlyOwner {
        _mint(recipient, amount);
    }
}
