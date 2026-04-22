// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

/// ============ Imports ============

import "./DSTestExtended.sol";
import "./SeismicFaucetUser.sol";
import "./MockSUSDC.sol";
import "../../SeismicFaucet.sol";

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
        SUSDC = new MockSUSDC();
        FAUCET = new SeismicFaucet(address(SUSDC));

        // Seed faucet with SUSDC
        SUSDC.mint(address(FAUCET), suint256(FAUCET_SEED));

        ALICE = new SeismicFaucetUser(FAUCET);
        BOB = new SeismicFaucetUser(FAUCET);

        FAUCET.updateSuperOperator(address(ALICE), true);
    }
}
