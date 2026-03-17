// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

/// ============ Imports ============

import "./DSTestExtended.sol"; // DSTestExtended
import "./SeismicFaucetUser.sol"; // Faucet user
import "../../SeismicFaucet.sol"; // SeismicFaucet

contract SeismicFaucetTest is DSTestExtended {
    /// ============ Storage ============

    /// @dev SeismicFaucet contract
    SeismicFaucet internal FAUCET;
    /// @dev User: Alice (default super operator)
    SeismicFaucetUser internal ALICE;
    /// @dev User: Bob
    SeismicFaucetUser internal BOB;

    /// ============ Setup test suite ============

    function setUp() public virtual {
        // Create faucet
        FAUCET = new SeismicFaucet();

        // Fund faucet with ETH
        (bool success,) = payable(address(FAUCET)).call{value: 100 ether}("");
        require(success, "Failed funding faucet with ETH");

        // Setup faucet users
        ALICE = new SeismicFaucetUser(FAUCET);
        BOB = new SeismicFaucetUser(FAUCET);

        // Make Alice superOperator
        FAUCET.updateSuperOperator(address(ALICE), true);
    }
}
