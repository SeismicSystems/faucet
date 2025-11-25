// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

/// @title Seismic Faucet
/// @author Ameya Deshmukh
/// @dev Based on Anish Agnihotri's MultiFaucet (https://github.com/Anish-Agnihotri/MultiFaucet)
/// @dev Pruned down version that drips only ETH instead of multiple tokens
/// @notice Drips ETH only
contract SeismicFaucet {
    /// ============ Mutable storage ============

    /// @notice ETH to disperse
    uint256 public ETH_AMOUNT = 5e17; // 0.5 ETH
    /// @notice Addresses of approved operators
    mapping(address => bool) public approvedOperators;
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

    /// ============ Events ============

    /// @notice Emitted after faucet drips to a recipient
    /// @param recipient address dripped to
    event FaucetDripped(address indexed recipient);

    /// @notice Emitted after faucet drained to a recipient
    /// @param recipient address drained to
    event FaucetDrained(address indexed recipient);

    /// @notice Emitted after operator status is updated
    /// @param operator address being updated
    /// @param status new operator status
    event OperatorUpdated(address indexed operator, bool status);

    /// @notice Emitted after super operator is updated
    /// @param operator address being updated
    /// @param status new operator status
    event SuperOperatorUpdated(address indexed operator, bool status);

    /// ============ Constructor ============

    /// @notice Creates a new MultiFaucet contract
    constructor() {
        superOperators[msg.sender] = true;
    }

    /// ============ Functions ============

    /// @notice Drips ETH to recipient
    /// @param _recipient to drip tokens to
    function drip(address _recipient) external isApprovedOperator {
        // Drip Ether
        (bool sent,) = _recipient.call{value: ETH_AMOUNT}("");
        require(sent, "Failed dripping ETH");

        emit FaucetDripped(_recipient);
    }

    /// @notice Returns number of available ETH drips
    /// @return ethDrips — available Ether drips
    function availableDrips() public view returns (uint256 ethDrips) {
        ethDrips = address(this).balance / ETH_AMOUNT;
    }

    /// @notice Allows super operator to drain contract of ETH
    /// @param _recipient to send drained ETH to
    function drain(address _recipient) external isSuperOperator {
        // Drain all Ether
        (bool sent,) = _recipient.call{value: address(this).balance}("");
        require(sent, "Failed draining ETH");

        emit FaucetDrained(_recipient);
    }

    /// @notice Allows super operator to update approved drip operator status
    /// @param _operator address to update
    /// @param _status of operator to toggle (true == allowed to drip)
    function updateApprovedOperator(address _operator, bool _status) external isSuperOperator {
        approvedOperators[_operator] = _status;
        emit OperatorUpdated(_operator, _status);
    }

    /// @notice Allows super operator to update super operator
    /// @param _operator address to update
    /// @param _status of operator to toggle (true === is super operator)
    function updateSuperOperator(address _operator, bool _status) external isSuperOperator {
        superOperators[_operator] = _status;
        emit SuperOperatorUpdated(_operator, _status);
    }

    /// @notice Allows super operator to update drip amount
    /// @param _ethAmount ETH to drip
    function updateDripAmount(uint256 _ethAmount) external isSuperOperator {
        ETH_AMOUNT = _ethAmount;
    }

    /// @notice Allows receiving ETH
    receive() external payable {}
}
