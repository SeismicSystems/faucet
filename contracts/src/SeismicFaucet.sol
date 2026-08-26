// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

import {ISRC20} from "seismic-std-lib/interfaces/ISRC20.sol";

/// @title Seismic Faucet
/// @author Ameya Deshmukh
/// @dev Based on Anish Agnihotri's MultiFaucet (https://github.com/Anish-Agnihotri/MultiFaucet)
/// @notice Drips SUSDC (shielded USDC, SRC20) across Seismic testnet networks
contract SeismicFaucet {
    /// ============ Immutable storage ============

    /// @notice SUSDC token this faucet drips
    ISRC20 public immutable susdc;

    /// ============ Mutable storage ============

    /// @notice SUSDC to disperse (6 decimals)
    uint256 public USDC_AMOUNT = 10e6; // 10 SUSDC
    /// @notice SUSDC to disperse to developers
    uint256 public DEVELOPER_USDC_AMOUNT = 50e6; // 50 SUSDC
    /// @notice SUSDC to disperse to whitelisted users
    uint256 public WHITELIST_USDC_AMOUNT = 250e6; // 250 SUSDC
    /// @notice Maximum SUSDC allowed in one exact operator transfer
    uint256 public MAX_EXACT_TRANSFER_AMOUNT = 250e6; // 250 SUSDC
    /// @notice Addresses of approved operators
    mapping(address => bool) public approvedOperators;
    /// @notice Addresses allowed to submit exact machine transfers
    mapping(address => bool) public machineOperators;
    /// @notice Addresses of super operators
    mapping(address => bool) public superOperators;

    /// ============ Modifiers ============

    /// @notice Requires sender to be contract super operator
    modifier isSuperOperator() {
        // Ensure sender is super operator
        require(superOperators[msg.sender], "Not super operator");
        _;
    }

    /// @notice Requires sender to be contract approved operator
    modifier isApprovedOperator() {
        require(approvedOperators[msg.sender] || superOperators[msg.sender], "Not approved operator");
        _;
    }

    /// @notice Requires sender to be a machine or super operator
    modifier isMachineOperator() {
        require(machineOperators[msg.sender] || superOperators[msg.sender], "Not machine operator");
        _;
    }

    /// ============ Events ============

    /// @notice Emitted after faucet drips to a recipient
    /// @param recipient address dripped to
    event FaucetDripped(address indexed recipient);

    /// @notice Emitted after an exact operator transfer
    /// @param recipient address transferred to
    event FaucetTransferred(address indexed recipient);

    /// @notice Emitted after faucet drained to a recipient
    /// @param recipient address drained to
    event FaucetDrained(address indexed recipient);

    /// @notice Emitted after operator status is updated
    /// @param operator address being updated
    /// @param status new operator status
    event OperatorUpdated(address indexed operator, bool status);

    /// @notice Emitted after machine operator status is updated
    /// @param operator address being updated
    /// @param status new operator status
    event MachineOperatorUpdated(address indexed operator, bool status);

    /// @notice Emitted after super operator is updated
    /// @param operator address being updated
    /// @param status new operator status
    event SuperOperatorUpdated(address indexed operator, bool status);

    /// ============ Constructor ============

    /// @notice Creates a new SeismicFaucet contract
    /// @param _susdc address of the deployed SUSDC (SRC20) token to drip
    constructor(address _susdc) {
        susdc = ISRC20(_susdc);
        superOperators[msg.sender] = true;
    }

    /// ============ Functions ============

    /// @notice Drips SUSDC to recipient
    /// @param _recipient to drip tokens to
    function drip(address _recipient) external isApprovedOperator {
        // Drip SUSDC
        require(susdc.transfer(_recipient, suint256(USDC_AMOUNT)), "Failed dripping SUSDC");

        emit FaucetDripped(_recipient);
    }

    /// @notice Drips SUSDC to developer recipient
    /// @param _recipient to drip tokens to
    function dripDeveloper(address _recipient) external isApprovedOperator {
        // Drip SUSDC (developer amount)
        require(susdc.transfer(_recipient, suint256(DEVELOPER_USDC_AMOUNT)), "Failed dripping SUSDC");

        emit FaucetDripped(_recipient);
    }

    /// @notice Drips larger SUSDC amount to whitelisted recipient
    /// @param _recipient to drip tokens to
    function dripWhitelist(address _recipient) external isApprovedOperator {
        // Drip SUSDC (whitelist amount)
        require(susdc.transfer(_recipient, suint256(WHITELIST_USDC_AMOUNT)), "Failed dripping SUSDC");

        emit FaucetDripped(_recipient);
    }

    /// @notice Transfers an exact SUSDC amount to a recipient
    /// @param _recipient recipient address
    /// @param _amount SUSDC amount in 6-decimal base units
    function transferExact(address _recipient, uint256 _amount) external isMachineOperator {
        require(_recipient != address(0), "Invalid recipient");
        require(_amount > 0 && _amount <= MAX_EXACT_TRANSFER_AMOUNT, "Invalid transfer amount");
        require(susdc.transfer(_recipient, suint256(_amount)), "Failed transferring SUSDC");

        emit FaucetTransferred(_recipient);
    }

    /// @notice Allows super operator to drain contract of SUSDC
    /// @param _recipient to send drained SUSDC to
    /// @dev `susdc.balance()` on SRC20 returns `balances[msg.sender]`, i.e. this contract's own balance
    function drain(address _recipient) external isSuperOperator {
        // Drain all SUSDC
        uint256 bal = susdc.balance();
        require(susdc.transfer(_recipient, suint256(bal)), "Failed draining SUSDC");

        emit FaucetDrained(_recipient);
    }

    /// @notice Allows super operator to update approved drip operator status
    /// @param _operator address to update
    /// @param _status of operator to toggle (true == allowed to drip)
    function updateApprovedOperator(address _operator, bool _status) external isSuperOperator {
        approvedOperators[_operator] = _status;
        emit OperatorUpdated(_operator, _status);
    }

    /// @notice Allows super operator to update exact-transfer operator status
    /// @param _operator address to update
    /// @param _status whether the operator may submit exact transfers
    function updateMachineOperator(address _operator, bool _status) external isSuperOperator {
        machineOperators[_operator] = _status;
        emit MachineOperatorUpdated(_operator, _status);
    }

    /// @notice Allows super operator to update super operator
    /// @param _operator address to update
    /// @param _status of operator to toggle (true === is super operator)
    function updateSuperOperator(address _operator, bool _status) external isSuperOperator {
        superOperators[_operator] = _status;
        emit SuperOperatorUpdated(_operator, _status);
    }

    /// @notice Allows super operator to update drip amount
    /// @param _amount SUSDC to drip (6 decimals)
    function updateDripAmount(uint256 _amount) external isSuperOperator {
        USDC_AMOUNT = _amount;
    }

    /// @notice Allows super operator to update developer drip amount
    /// @param _amount SUSDC to drip to developers (6 decimals)
    function updateDeveloperDripAmount(uint256 _amount) external isSuperOperator {
        DEVELOPER_USDC_AMOUNT = _amount;
    }

    /// @notice Allows super operator to update whitelist drip amount
    /// @param _amount SUSDC to drip to whitelisted users (6 decimals)
    function updateWhitelistDripAmount(uint256 _amount) external isSuperOperator {
        WHITELIST_USDC_AMOUNT = _amount;
    }

    /// @notice Allows super operator to update the exact transfer ceiling
    /// @param _amount maximum SUSDC per transfer in 6-decimal base units
    function updateMaxExactTransferAmount(uint256 _amount) external isSuperOperator {
        require(_amount > 0, "Invalid transfer amount");
        MAX_EXACT_TRANSFER_AMOUNT = _amount;
    }
}
