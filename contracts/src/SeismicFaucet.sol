// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

import {ISRC20} from "seismic-std-lib/interfaces/ISRC20.sol";

/// @title Seismic Faucet
/// @author Ameya Deshmukh
/// @dev Based on Anish Agnihotri's MultiFaucet (https://github.com/Anish-Agnihotri/MultiFaucet)
/// @notice Drips SUSDC (shielded USDC on Seismic)
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
    /// @notice Addresses of approved operators
    mapping(address => bool) public approvedOperators;
    /// @notice Addresses of super operators
    mapping(address => bool) public superOperators;

    /// ============ Modifiers ============

    /// @notice Requires sender to be contract super operator
    modifier isSuperOperator() {
        require(superOperators[msg.sender], "Not super operator");
        _;
    }

    /// @notice Requires sender to be contract approved operator
    modifier isApprovedOperator() {
        require(approvedOperators[msg.sender] || superOperators[msg.sender], "Not approved operator");
        _;
    }

    /// ============ Events ============

    event FaucetDripped(address indexed recipient);
    event FaucetDrained(address indexed recipient);
    event OperatorUpdated(address indexed operator, bool status);
    event SuperOperatorUpdated(address indexed operator, bool status);

    /// ============ Constructor ============

    constructor(address _susdc) {
        susdc = ISRC20(_susdc);
        superOperators[msg.sender] = true;
    }

    /// ============ Functions ============

    /// @notice Drips SUSDC to recipient
    function drip(address _recipient) external isApprovedOperator {
        require(susdc.transfer(_recipient, suint256(USDC_AMOUNT)), "Failed dripping SUSDC");
        emit FaucetDripped(_recipient);
    }

    /// @notice Drips SUSDC to developer recipient
    function dripDeveloper(address _recipient) external isApprovedOperator {
        require(susdc.transfer(_recipient, suint256(DEVELOPER_USDC_AMOUNT)), "Failed dripping SUSDC");
        emit FaucetDripped(_recipient);
    }

    /// @notice Drips larger SUSDC amount to whitelisted recipient
    function dripWhitelist(address _recipient) external isApprovedOperator {
        require(susdc.transfer(_recipient, suint256(WHITELIST_USDC_AMOUNT)), "Failed dripping SUSDC");
        emit FaucetDripped(_recipient);
    }

    /// @notice Allows super operator to drain contract of SUSDC
    function drain(address _recipient) external isSuperOperator {
        uint256 bal = susdc.balance();
        require(susdc.transfer(_recipient, suint256(bal)), "Failed draining SUSDC");
        emit FaucetDrained(_recipient);
    }

    function updateApprovedOperator(address _operator, bool _status) external isSuperOperator {
        approvedOperators[_operator] = _status;
        emit OperatorUpdated(_operator, _status);
    }

    function updateSuperOperator(address _operator, bool _status) external isSuperOperator {
        superOperators[_operator] = _status;
        emit SuperOperatorUpdated(_operator, _status);
    }

    function updateDripAmount(uint256 _amount) external isSuperOperator {
        USDC_AMOUNT = _amount;
    }

    function updateDeveloperDripAmount(uint256 _amount) external isSuperOperator {
        DEVELOPER_USDC_AMOUNT = _amount;
    }

    function updateWhitelistDripAmount(uint256 _amount) external isSuperOperator {
        WHITELIST_USDC_AMOUNT = _amount;
    }
}
