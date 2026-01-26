// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

/// ============ Imports ============

import "./utils/SeismicFaucetTest.sol"; // SeismicFaucet ds-test

/// ============ Libraries ============

library Errors {
    string constant NotSuperOperator = "Not super operator";
    string constant NotApprovedOperator = "Not approved operator";
}

/// ============ Functionality testing ============

contract Tests is SeismicFaucetTest {
    /// @notice Allow dripping ETH to recipient, if super operator
    function testDrip() public {
        // Bob before balance
        uint256 bobETHBalanceBefore = BOB.ETHBalance();

        // Alice drips to bob
        ALICE.drip(address(BOB));

        // Bob after balance - should receive 1 ETH
        assertEq(BOB.ETHBalance(), bobETHBalanceBefore + 1 ether);
    }

    /// @notice Prevent dripping if not approved operator
    function testCannotDripIfNotOperator() public {
        assertErrorFunctionWithAddress(BOB.drip, address(ALICE), Errors.NotApprovedOperator);
    }

    /// @notice Can add approved operator and they can drip
    function testAddApprovedOperator() public {
        // Alice adds Bob as approved operator
        ALICE.updateApprovedOperator(address(BOB), true);

        // Ensure Bob is an approved operator
        assertTrue(FAUCET.approvedOperators(address(BOB)));

        // Ensure Bob can drip
        BOB.drip(address(ALICE));
    }

    /// @notice Can remove approved operator and they can't drip
    function testRemoveApprovedOperator() public {
        // Alice adds Bob as approved operator
        ALICE.updateApprovedOperator(address(BOB), true);
        assertTrue(FAUCET.approvedOperators(address(BOB)));

        // Alice removes Bob as approved operator
        ALICE.updateApprovedOperator(address(BOB), false);
        assertTrue(!FAUCET.approvedOperators(address(BOB)));

        // Bob can no longer drip
        assertErrorFunctionWithAddress(BOB.drip, address(ALICE), Errors.NotApprovedOperator);
    }

    /// @notice Can update super operator
    function testUpdateSuperOperator() public {
        // Alice gives super operatorship to BOB
        ALICE.updateSuperOperator(address(BOB), true);

        // Verify Bob is now a super operator
        assertTrue(FAUCET.superOperators(address(BOB)));

        // Alice removes her super operatorship
        ALICE.updateSuperOperator(address(ALICE), false);

        // Alice can no longer drip
        assertErrorFunctionWithAddress(ALICE.drip, address(BOB), Errors.NotApprovedOperator);

        // Bob can add Alice to super operators
        BOB.updateApprovedOperator(address(ALICE), true);
        assertTrue(FAUCET.approvedOperators(address(ALICE)));

        // Alice can now drip
        ALICE.drip(address(BOB));

        // Alice can still not update super operator
        assertErrorFunctionWithAddressAndBool(ALICE.updateSuperOperator, address(ALICE), true, Errors.NotSuperOperator);
    }

    /// @notice Can drain contract if super operator
    function testCanDrainFaucet() public {
        // Bob before balance
        uint256 bobETHBalanceBefore = BOB.ETHBalance();

        // Alice drains to bob
        ALICE.drain(address(BOB));

        // Bob after balance - should receive all 100 ETH from faucet
        assertEq(BOB.ETHBalance(), bobETHBalanceBefore + 100 ether);
    }

    /// @notice Cannot drain contract if not super operator
    function testCannotDrainIfNotSuperOperator() public {
        assertErrorFunctionWithAddress(BOB.drain, address(BOB), Errors.NotSuperOperator);
    }

    /// @notice Returns correct number of available ETH drips
    function testCorrectDripCount() public {
        uint256 ethDrips = FAUCET.availableDrips();
        assertEq(ethDrips, 100); // 100 ETH / 1 ETH per drip = 100 drips
    }

    /// @notice Allows super operators to update drip amount
    function testAllowsUpdatingDripAmount() public {
        // Bob before balance
        uint256 bobETHBalanceBefore = BOB.ETHBalance();

        // Alice updates drip amount to 0.5 ETH
        ALICE.updateDripAmount(0.5 ether);

        // Alice drips to bob
        ALICE.drip(address(BOB));

        // Bob after balance - should receive 0.5 ETH
        assertEq(BOB.ETHBalance(), bobETHBalanceBefore + 0.5 ether);
    }

    /// @notice Allow dripping developer ETH amount to recipient, if super operator
    function testDripDeveloper() public {
        // Bob before balance
        uint256 bobETHBalanceBefore = BOB.ETHBalance();

        // Alice drips developer amount to bob
        ALICE.dripDeveloper(address(BOB));

        // Bob after balance - should receive 2 ETH (default developer amount)
        assertEq(BOB.ETHBalance(), bobETHBalanceBefore + 2 ether);
    }

    /// @notice Prevent developer dripping if not approved operator
    function testCannotDripDeveloperIfNotOperator() public {
        assertErrorFunctionWithAddress(BOB.dripDeveloper, address(ALICE), Errors.NotApprovedOperator);
    }

    /// @notice Approved operator can drip developer amount
    function testApprovedOperatorCanDripDeveloper() public {
        // Alice adds Bob as approved operator
        ALICE.updateApprovedOperator(address(BOB), true);

        // Alice before balance
        uint256 aliceETHBalanceBefore = ALICE.ETHBalance();

        // Bob drips developer amount to Alice
        BOB.dripDeveloper(address(ALICE));

        // Alice after balance - should receive 2 ETH
        assertEq(ALICE.ETHBalance(), aliceETHBalanceBefore + 2 ether);
    }

    /// @notice Allows super operators to update developer drip amount
    function testAllowsUpdatingDeveloperDripAmount() public {
        // Bob before balance
        uint256 bobETHBalanceBefore = BOB.ETHBalance();

        // Alice updates developer drip amount to 3 ETH
        ALICE.updateDeveloperDripAmount(3 ether);

        // Verify the amount was updated
        assertEq(FAUCET.DEVELOPER_ETH_AMOUNT(), 3 ether);

        // Alice drips developer amount to bob
        ALICE.dripDeveloper(address(BOB));

        // Bob after balance - should receive 3 ETH
        assertEq(BOB.ETHBalance(), bobETHBalanceBefore + 3 ether);
    }

    /// @notice Non-super operator cannot update developer drip amount
    function testCannotUpdateDeveloperDripAmountIfNotSuperOperator() public {
        assertErrorFunctionWithUint256(BOB.updateDeveloperDripAmount, 3 ether, Errors.NotSuperOperator);
    }

    /// @notice Default developer ETH amount is 2 ETH
    function testDefaultDeveloperAmount() public {
        assertEq(FAUCET.DEVELOPER_ETH_AMOUNT(), 2 ether);
    }

    /// @notice Allow dripping whitelist ETH amount to recipient, if super operator
    function testDripWhitelist() public {
        // Bob before balance
        uint256 bobETHBalanceBefore = BOB.ETHBalance();

        // Alice drips whitelist amount to bob
        ALICE.dripWhitelist(address(BOB));

        // Bob after balance - should receive 10 ETH (default whitelist amount)
        assertEq(BOB.ETHBalance(), bobETHBalanceBefore + 10 ether);
    }

    /// @notice Prevent whitelist dripping if not approved operator
    function testCannotDripWhitelistIfNotOperator() public {
        assertErrorFunctionWithAddress(BOB.dripWhitelist, address(ALICE), Errors.NotApprovedOperator);
    }

    /// @notice Approved operator can drip whitelist amount
    function testApprovedOperatorCanDripWhitelist() public {
        // Alice adds Bob as approved operator
        ALICE.updateApprovedOperator(address(BOB), true);

        // Bob before balance (we'll drip to Alice)
        uint256 aliceETHBalanceBefore = ALICE.ETHBalance();

        // Bob drips whitelist amount to Alice
        BOB.dripWhitelist(address(ALICE));

        // Alice after balance - should receive 10 ETH
        assertEq(ALICE.ETHBalance(), aliceETHBalanceBefore + 10 ether);
    }

    /// @notice Allows super operators to update whitelist drip amount
    function testAllowsUpdatingWhitelistDripAmount() public {
        // Bob before balance
        uint256 bobETHBalanceBefore = BOB.ETHBalance();

        // Alice updates whitelist drip amount to 5 ETH
        ALICE.updateWhitelistDripAmount(5 ether);

        // Verify the amount was updated
        assertEq(FAUCET.WHITELIST_ETH_AMOUNT(), 5 ether);

        // Alice drips whitelist amount to bob
        ALICE.dripWhitelist(address(BOB));

        // Bob after balance - should receive 5 ETH
        assertEq(BOB.ETHBalance(), bobETHBalanceBefore + 5 ether);
    }

    /// @notice Non-super operator cannot update whitelist drip amount
    function testCannotUpdateWhitelistDripAmountIfNotSuperOperator() public {
        assertErrorFunctionWithUint256(BOB.updateWhitelistDripAmount, 5 ether, Errors.NotSuperOperator);
    }

    /// @notice Default whitelist ETH amount is 10 ETH
    function testDefaultWhitelistAmount() public {
        assertEq(FAUCET.WHITELIST_ETH_AMOUNT(), 10 ether);
    }
}
