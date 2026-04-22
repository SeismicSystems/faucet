// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

/// ============ Imports ============

import "./DSTestExtended.sol"; // DSTestExtended
import "./SeismicFaucetUser.sol"; // Faucet user
import "./MockSUSDC.sol"; // Mock SUSDC token
import "../../SeismicFaucet.sol"; // SeismicFaucet

contract SeismicFaucetTest is DSTestExtended {
    /// @dev Faucet funding amount in tests (6 decimals)
    uint256 internal constant FAUCET_SEED = 100_000e6; // 100,000 SUSDC

    /// ============ Storage ============

    /// @dev Mock SUSDC token
    MockSUSDC internal SUSDC;
    /// @dev SeismicFaucet contract
    SeismicFaucet internal FAUCET;
    /// @dev User: Alice (default super operator)
    SeismicFaucetUser internal ALICE;
    /// @dev User: Bob
    SeismicFaucetUser internal BOB;

    /// ============ Setup test suite ============

    function setUp() public virtual {
        // Deploy mock SUSDC
        SUSDC = new MockSUSDC();

        // Create faucet
        FAUCET = new SeismicFaucet(address(SUSDC));

        // Fund faucet with SUSDC (test admin mints directly to faucet)
        SUSDC.mint(address(FAUCET), suint256(FAUCET_SEED));

        // Setup faucet users
        ALICE = new SeismicFaucetUser(FAUCET);
        BOB = new SeismicFaucetUser(FAUCET);

        // Make Alice superOperator
        FAUCET.updateSuperOperator(address(ALICE), true);
    }
}
