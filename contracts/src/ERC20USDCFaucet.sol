// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

/// @notice Independently deployed machine faucet for standard ERC20 testnet USDC.
contract ERC20USDCFaucet {
    using SafeERC20 for IERC20;

    uint256 public MAX_EXACT_TRANSFER_AMOUNT = 250e6;

    IERC20 public immutable usdc;
    mapping(address => bool) public machineOperators;
    mapping(address => bool) public superOperators;

    event FaucetTransferred(address indexed recipient, uint256 amount);
    event FaucetDrained(address indexed recipient, uint256 amount);
    event MachineOperatorUpdated(address indexed operator, bool status);
    event SuperOperatorUpdated(address indexed operator, bool status);

    modifier isMachineOperator() {
        require(machineOperators[msg.sender] || superOperators[msg.sender], "Not machine operator");
        _;
    }

    modifier isSuperOperator() {
        require(superOperators[msg.sender], "Not super operator");
        _;
    }

    constructor(address usdcAddress, address machineOperator) {
        require(usdcAddress != address(0), "Invalid USDC");
        require(machineOperator != address(0), "Invalid machine operator");

        usdc = IERC20(usdcAddress);
        superOperators[msg.sender] = true;
        machineOperators[machineOperator] = true;
    }

    function transferExact(address recipient, uint256 amount) external isMachineOperator {
        require(recipient != address(0), "Invalid recipient");
        require(amount > 0 && amount <= MAX_EXACT_TRANSFER_AMOUNT, "Invalid transfer amount");

        usdc.safeTransfer(recipient, amount);
        emit FaucetTransferred(recipient, amount);
    }

    function drain(address recipient) external isSuperOperator {
        require(recipient != address(0), "Invalid recipient");

        uint256 balance = usdc.balanceOf(address(this));
        usdc.safeTransfer(recipient, balance);
        emit FaucetDrained(recipient, balance);
    }

    function updateMachineOperator(address operator, bool status) external isSuperOperator {
        require(operator != address(0), "Invalid operator");

        machineOperators[operator] = status;
        emit MachineOperatorUpdated(operator, status);
    }

    function updateSuperOperator(address operator, bool status) external isSuperOperator {
        require(operator != address(0), "Invalid operator");

        superOperators[operator] = status;
        emit SuperOperatorUpdated(operator, status);
    }

    function updateMaxExactTransferAmount(uint256 amount) external isSuperOperator {
        require(amount > 0, "Invalid transfer amount");
        MAX_EXACT_TRANSFER_AMOUNT = amount;
    }
}
