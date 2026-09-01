// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

import {Test} from "forge-std/Test.sol";
import {ERC20USDCFaucet} from "../ERC20USDCFaucet.sol";
import {TestnetUSDC} from "../TestnetUSDC.sol";

contract ERC20USDCFaucetTest is Test {
    uint256 internal constant FAUCET_SEED = 1_000_000e6;
    uint256 internal constant TRANSFER_AMOUNT = 10e6;
    address internal constant MACHINE_OPERATOR = address(0x1001);
    address internal constant RECIPIENT = address(0x2002);
    address internal constant RESERVE = address(0x3003);

    TestnetUSDC internal usdc;
    ERC20USDCFaucet internal faucet;

    function setUp() public {
        usdc = new TestnetUSDC(address(this));
        faucet = new ERC20USDCFaucet(address(usdc), MACHINE_OPERATOR);
        usdc.mint(address(faucet), FAUCET_SEED);
        usdc.transferOwnership(RESERVE);
        faucet.updateSuperOperator(RESERVE, true);
        faucet.updateSuperOperator(address(this), false);
    }

    function testTokenIsStandardSixDecimalERC20() public {
        assertEq(usdc.name(), "Seismic Testnet USDC");
        assertEq(usdc.symbol(), "USDC");
        assertEq(usdc.decimals(), 6);

        vm.prank(RESERVE);
        usdc.mint(address(this), TRANSFER_AMOUNT);
        usdc.approve(RECIPIENT, TRANSFER_AMOUNT);
        vm.prank(RECIPIENT);
        usdc.transferFrom(address(this), RECIPIENT, TRANSFER_AMOUNT);
        assertEq(usdc.balanceOf(RECIPIENT), TRANSFER_AMOUNT);
    }

    function testOnlyOwnerCanMint() public {
        vm.prank(MACHINE_OPERATOR);
        vm.expectRevert("Ownable: caller is not the owner");
        usdc.mint(MACHINE_OPERATOR, TRANSFER_AMOUNT);
    }

    function testReserveOwnsRecoveryAndMachineHasNoAdminRole() public view {
        assertEq(usdc.owner(), RESERVE);
        assertTrue(faucet.superOperators(RESERVE));
        assertFalse(faucet.superOperators(MACHINE_OPERATOR));
        assertFalse(faucet.superOperators(address(this)));
    }

    function testMachineOperatorTransfersExactUSDC() public {
        vm.prank(MACHINE_OPERATOR);
        faucet.transferExact(RECIPIENT, TRANSFER_AMOUNT);

        assertEq(usdc.balanceOf(RECIPIENT), TRANSFER_AMOUNT);
        assertEq(usdc.balanceOf(address(faucet)), FAUCET_SEED - TRANSFER_AMOUNT);
    }

    function testUnapprovedAccountCannotTransfer() public {
        vm.prank(RECIPIENT);
        vm.expectRevert("Not machine operator");
        faucet.transferExact(RECIPIENT, TRANSFER_AMOUNT);
    }

    function testTransferValidatesRecipientAndLimit() public {
        uint256 amountAboveLimit = faucet.MAX_EXACT_TRANSFER_AMOUNT() + 1;
        vm.startPrank(MACHINE_OPERATOR);
        vm.expectRevert("Invalid recipient");
        faucet.transferExact(address(0), TRANSFER_AMOUNT);
        vm.expectRevert("Invalid transfer amount");
        faucet.transferExact(RECIPIENT, 0);
        vm.expectRevert("Invalid transfer amount");
        faucet.transferExact(RECIPIENT, amountAboveLimit);
        vm.stopPrank();
    }

    function testSuperOperatorControlsRolesLimitsAndDrain() public {
        address secondOperator = address(0x4004);
        vm.startPrank(RESERVE);
        faucet.updateMachineOperator(secondOperator, true);
        faucet.updateMaxExactTransferAmount(500e6);

        assertTrue(faucet.machineOperators(secondOperator));
        assertEq(faucet.MAX_EXACT_TRANSFER_AMOUNT(), 500e6);

        faucet.drain(RECIPIENT);
        vm.stopPrank();
        assertEq(usdc.balanceOf(RECIPIENT), FAUCET_SEED);
        assertEq(usdc.balanceOf(address(faucet)), 0);
    }
}
