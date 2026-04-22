// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

import "./utils/SeismicFaucetTest.sol";

library Errors {
    string constant NotSuperOperator = "Not super operator";
    string constant NotApprovedOperator = "Not approved operator";
}

/// @notice SUSDC amounts for tests (6 decimals)
library Amounts {
    uint256 constant REGULAR = 10e6;
    uint256 constant DEVELOPER = 50e6;
    uint256 constant WHITELIST = 250e6;
}

contract Tests is SeismicFaucetTest {
    function testDrip() public {
        uint256 bobBefore = BOB.SUSDCBalance();
        ALICE.drip(address(BOB));
        assertEq(BOB.SUSDCBalance(), bobBefore + Amounts.REGULAR);
    }

    function testCannotDripIfNotOperator() public {
        assertErrorFunctionWithAddress(BOB.drip, address(ALICE), Errors.NotApprovedOperator);
    }

    function testAddApprovedOperator() public {
        ALICE.updateApprovedOperator(address(BOB), true);
        assertTrue(FAUCET.approvedOperators(address(BOB)));
        BOB.drip(address(ALICE));
    }

    function testRemoveApprovedOperator() public {
        ALICE.updateApprovedOperator(address(BOB), true);
        assertTrue(FAUCET.approvedOperators(address(BOB)));

        ALICE.updateApprovedOperator(address(BOB), false);
        assertTrue(!FAUCET.approvedOperators(address(BOB)));

        assertErrorFunctionWithAddress(BOB.drip, address(ALICE), Errors.NotApprovedOperator);
    }

    function testUpdateSuperOperator() public {
        ALICE.updateSuperOperator(address(BOB), true);
        assertTrue(FAUCET.superOperators(address(BOB)));

        ALICE.updateSuperOperator(address(ALICE), false);
        assertErrorFunctionWithAddress(ALICE.drip, address(BOB), Errors.NotApprovedOperator);

        BOB.updateApprovedOperator(address(ALICE), true);
        assertTrue(FAUCET.approvedOperators(address(ALICE)));

        ALICE.drip(address(BOB));

        assertErrorFunctionWithAddressAndBool(ALICE.updateSuperOperator, address(ALICE), true, Errors.NotSuperOperator);
    }

    function testCanDrainFaucet() public {
        uint256 bobBefore = BOB.SUSDCBalance();
        ALICE.drain(address(BOB));
        assertEq(BOB.SUSDCBalance(), bobBefore + FAUCET_SEED);
    }

    function testCannotDrainIfNotSuperOperator() public {
        assertErrorFunctionWithAddress(BOB.drain, address(BOB), Errors.NotSuperOperator);
    }

    function testAllowsUpdatingDripAmount() public {
        uint256 bobBefore = BOB.SUSDCBalance();
        uint256 newAmount = 5e6; // 5 SUSDC

        ALICE.updateDripAmount(newAmount);
        ALICE.drip(address(BOB));

        assertEq(BOB.SUSDCBalance(), bobBefore + newAmount);
    }

    function testDripDeveloper() public {
        uint256 bobBefore = BOB.SUSDCBalance();
        ALICE.dripDeveloper(address(BOB));
        assertEq(BOB.SUSDCBalance(), bobBefore + Amounts.DEVELOPER);
    }

    function testCannotDripDeveloperIfNotOperator() public {
        assertErrorFunctionWithAddress(BOB.dripDeveloper, address(ALICE), Errors.NotApprovedOperator);
    }

    function testApprovedOperatorCanDripDeveloper() public {
        ALICE.updateApprovedOperator(address(BOB), true);

        uint256 aliceBefore = ALICE.SUSDCBalance();
        BOB.dripDeveloper(address(ALICE));

        assertEq(ALICE.SUSDCBalance(), aliceBefore + Amounts.DEVELOPER);
    }

    function testAllowsUpdatingDeveloperDripAmount() public {
        uint256 bobBefore = BOB.SUSDCBalance();
        uint256 newAmount = 100e6; // 100 SUSDC

        ALICE.updateDeveloperDripAmount(newAmount);
        assertEq(FAUCET.DEVELOPER_USDC_AMOUNT(), newAmount);

        ALICE.dripDeveloper(address(BOB));
        assertEq(BOB.SUSDCBalance(), bobBefore + newAmount);
    }

    function testCannotUpdateDeveloperDripAmountIfNotSuperOperator() public {
        assertErrorFunctionWithUint256(BOB.updateDeveloperDripAmount, 100e6, Errors.NotSuperOperator);
    }

    function testDefaultDeveloperAmount() public {
        assertEq(FAUCET.DEVELOPER_USDC_AMOUNT(), Amounts.DEVELOPER);
    }

    function testDripWhitelist() public {
        uint256 bobBefore = BOB.SUSDCBalance();
        ALICE.dripWhitelist(address(BOB));
        assertEq(BOB.SUSDCBalance(), bobBefore + Amounts.WHITELIST);
    }

    function testCannotDripWhitelistIfNotOperator() public {
        assertErrorFunctionWithAddress(BOB.dripWhitelist, address(ALICE), Errors.NotApprovedOperator);
    }

    function testApprovedOperatorCanDripWhitelist() public {
        ALICE.updateApprovedOperator(address(BOB), true);

        uint256 aliceBefore = ALICE.SUSDCBalance();
        BOB.dripWhitelist(address(ALICE));

        assertEq(ALICE.SUSDCBalance(), aliceBefore + Amounts.WHITELIST);
    }

    function testAllowsUpdatingWhitelistDripAmount() public {
        uint256 bobBefore = BOB.SUSDCBalance();
        uint256 newAmount = 500e6; // 500 SUSDC

        ALICE.updateWhitelistDripAmount(newAmount);
        assertEq(FAUCET.WHITELIST_USDC_AMOUNT(), newAmount);

        ALICE.dripWhitelist(address(BOB));
        assertEq(BOB.SUSDCBalance(), bobBefore + newAmount);
    }

    function testCannotUpdateWhitelistDripAmountIfNotSuperOperator() public {
        assertErrorFunctionWithUint256(BOB.updateWhitelistDripAmount, 500e6, Errors.NotSuperOperator);
    }

    function testDefaultWhitelistAmount() public {
        assertEq(FAUCET.WHITELIST_USDC_AMOUNT(), Amounts.WHITELIST);
    }
}
