// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

import {SRC20} from "seismic-std-lib/SRC20.sol";

/// @notice Minimal concrete SRC20 used only for tests — exposes a public mint.
contract MockSUSDC is SRC20 {
    constructor() SRC20("Mock SUSDC", "mSUSDC", 6) {}

    function mint(address to, suint256 amount) external {
        _mint(to, amount);
    }
}
