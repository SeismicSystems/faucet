// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

/// ============ Imports ============

import "./utils/SeismicFaucetTest.sol"; // SeismicFaucet ds-test

/// ============ Libraries ============

library Errors {
    string constant NotSuperOperator = "Not super operator";
    string constant NotApprovedOperator = "Not approved operator";
}

/// @notice Default SUSDC drip amounts for tests (6 decimals)
library Amounts {
    uint256 constant REGULAR = 10e6;
    uint256 constant DEVELOPER = 50e6;
    uint256 constant WHITELIST = 250e6;
}

/// ============ Functionality testing ============

contract Tests is SeismicFaucetTest {
    /// @notice Allow dripping SUSDC to recipient, if super operator
    function testDrip() public {
        // Bob before balance
        uint256 bobBefore = BOB.SUSDCBalance();

        // Alice drips to Bob
        ALICE.drip(address(BOB));

        // Bob after balance - should receive default regular drip
        assertEq(BOB.SUSDCBalance(), bobBefore + Amounts.REGULAR);
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
        // Alice gives super operatorship to Bob
        ALICE.updateSuperOperator(address(BOB), true);

        // Verify Bob is now a super operator
        assertTrue(FAUCET.superOperators(address(BOB)));

        // Alice removes her super operatorship
        ALICE.updateSuperOperator(address(ALICE), false);

        // Alice can no longer drip
        assertErrorFunctionWithAddress(ALICE.drip, address(BOB), Errors.NotApprovedOperator);

        // Bob can add Alice to approved operators
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
        uint256 bobBefore = BOB.SUSDCBalance();

        // Alice drains to Bob
        ALICE.drain(address(BOB));

        // Bob after balance - should receive the full faucet seed
        assertEq(BOB.SUSDCBalance(), bobBefore + FAUCET_SEED);
    }

    /// @notice Cannot drain contract if not super operator
    function testCannotDrainIfNotSuperOperator() public {
        assertErrorFunctionWithAddress(BOB.drain, address(BOB), Errors.NotSuperOperator);
    }

    /// @notice Allows super operators to update drip amount
    function testAllowsUpdatingDripAmount() public {
        // Bob before balance
        uint256 bobBefore = BOB.SUSDCBalance();
        uint256 newAmount = 5e6; // 5 SUSDC

        // Alice updates drip amount
        ALICE.updateDripAmount(newAmount);

        // Alice drips to Bob
        ALICE.drip(address(BOB));

        // Bob after balance - should receive the new amount
        assertEq(BOB.SUSDCBalance(), bobBefore + newAmount);
    }

    /// @notice Allow dripping developer SUSDC amount to recipient, if super operator
    function testDripDeveloper() public {
        // Bob before balance
        uint256 bobBefore = BOB.SUSDCBalance();

        // Alice drips developer amount to Bob
        ALICE.dripDeveloper(address(BOB));

        // Bob after balance - should receive default developer amount
        assertEq(BOB.SUSDCBalance(), bobBefore + Amounts.DEVELOPER);
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
        uint256 aliceBefore = ALICE.SUSDCBalance();

        // Bob drips developer amount to Alice
        BOB.dripDeveloper(address(ALICE));

        // Alice after balance - should receive default developer amount
        assertEq(ALICE.SUSDCBalance(), aliceBefore + Amounts.DEVELOPER);
    }

    /// @notice Allows super operators to update developer drip amount
    function testAllowsUpdatingDeveloperDripAmount() public {
        // Bob before balance
        uint256 bobBefore = BOB.SUSDCBalance();
        uint256 newAmount = 100e6; // 100 SUSDC

        // Alice updates developer drip amount
        ALICE.updateDeveloperDripAmount(newAmount);

        // Verify the amount was updated
        assertEq(FAUCET.DEVELOPER_USDC_AMOUNT(), newAmount);

        // Alice drips developer amount to Bob
        ALICE.dripDeveloper(address(BOB));

        // Bob after balance - should receive the new amount
        assertEq(BOB.SUSDCBalance(), bobBefore + newAmount);
    }

    /// @notice Non-super operator cannot update developer drip amount
    function testCannotUpdateDeveloperDripAmountIfNotSuperOperator() public {
        assertErrorFunctionWithUint256(BOB.updateDeveloperDripAmount, 100e6, Errors.NotSuperOperator);
    }

    /// @notice Default developer SUSDC amount is 50 SUSDC
    function testDefaultDeveloperAmount() public {
        assertEq(FAUCET.DEVELOPER_USDC_AMOUNT(), Amounts.DEVELOPER);
    }

    /// @notice Allow dripping whitelist SUSDC amount to recipient, if super operator
    function testDripWhitelist() public {
        // Bob before balance
        uint256 bobBefore = BOB.SUSDCBalance();

        // Alice drips whitelist amount to Bob
        ALICE.dripWhitelist(address(BOB));

        // Bob after balance - should receive default whitelist amount
        assertEq(BOB.SUSDCBalance(), bobBefore + Amounts.WHITELIST);
    }

    /// @notice Prevent whitelist dripping if not approved operator
    function testCannotDripWhitelistIfNotOperator() public {
        assertErrorFunctionWithAddress(BOB.dripWhitelist, address(ALICE), Errors.NotApprovedOperator);
    }

    /// @notice Approved operator can drip whitelist amount
    function testApprovedOperatorCanDripWhitelist() public {
        // Alice adds Bob as approved operator
        ALICE.updateApprovedOperator(address(BOB), true);

        // Alice before balance
        uint256 aliceBefore = ALICE.SUSDCBalance();

        // Bob drips whitelist amount to Alice
        BOB.dripWhitelist(address(ALICE));

        // Alice after balance - should receive default whitelist amount
        assertEq(ALICE.SUSDCBalance(), aliceBefore + Amounts.WHITELIST);
    }

    /// @notice Allows super operators to update whitelist drip amount
    function testAllowsUpdatingWhitelistDripAmount() public {
        // Bob before balance
        uint256 bobBefore = BOB.SUSDCBalance();
        uint256 newAmount = 500e6; // 500 SUSDC

        // Alice updates whitelist drip amount
        ALICE.updateWhitelistDripAmount(newAmount);

        // Verify the amount was updated
        assertEq(FAUCET.WHITELIST_USDC_AMOUNT(), newAmount);

        // Alice drips whitelist amount to Bob
        ALICE.dripWhitelist(address(BOB));

        // Bob after balance - should receive the new amount
        assertEq(BOB.SUSDCBalance(), bobBefore + newAmount);
    }

    /// @notice Non-super operator cannot update whitelist drip amount
    function testCannotUpdateWhitelistDripAmountIfNotSuperOperator() public {
        assertErrorFunctionWithUint256(BOB.updateWhitelistDripAmount, 500e6, Errors.NotSuperOperator);
    }

    /// @notice Default whitelist SUSDC amount is 250 SUSDC
    function testDefaultWhitelistAmount() public {
        assertEq(FAUCET.WHITELIST_USDC_AMOUNT(), Amounts.WHITELIST);
    }
}
