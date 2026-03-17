// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

/// ============ Imports ============

import "../../SeismicFaucet.sol"; // SeismicFaucet

/// @title SeismicFaucetUser
/// @author Ameya Deshmukh
/// @dev Based on Anish Agnihotri's MultiFaucetUser (https://github.com/Anish-Agnihotri/MultiFaucet)
/// @notice Mock user to test interacting with SeismicFaucet
contract SeismicFaucetUser {
    /// ============ Immutable storage ============

    /// @dev Faucet contract
    SeismicFaucet internal immutable FAUCET;

    /// ============ Constructor ============

    /// @notice Creates a new SeismicFaucetUser
    /// @param _FAUCET contract
    constructor(SeismicFaucet _FAUCET) {
        FAUCET = _FAUCET;
    }

    /// ============ Helper functions ============

    /// @notice Returns ETH balance of user
    function ETHBalance() public view returns (uint256) {
        return address(this).balance;
    }

    /// ============ Inherited Functionality ============

    /// @notice Drips from faucet to recipient
    /// @param _recipient to drip to
    function drip(address _recipient) public {
        FAUCET.drip(_recipient);
    }

    /// @notice Drips developer amount from faucet to recipient
    /// @param _recipient to drip to
    function dripDeveloper(address _recipient) public {
        FAUCET.dripDeveloper(_recipient);
    }

    /// @notice Drips whitelist amount from faucet to recipient
    /// @param _recipient to drip to
    function dripWhitelist(address _recipient) public {
        FAUCET.dripWhitelist(_recipient);
    }

    /// @notice Drains faucet to a recipient address
    /// @param _recipient to drain to
    function drain(address _recipient) public {
        FAUCET.drain(_recipient);
    }

    /// @notice Adds or removes approved operator
    /// @param _operator address
    /// @param _status to update for operator (true == allowed to drip)
    function updateApprovedOperator(address _operator, bool _status) public {
        FAUCET.updateApprovedOperator(_operator, _status);
    }

    /// @notice Updates super operator
    /// @param _operator address
    /// @param _status of operator
    function updateSuperOperator(address _operator, bool _status) public {
        FAUCET.updateSuperOperator(_operator, _status);
    }

    /// @notice Updates drip amount
    /// @param _ethAmount ETH to drip
    function updateDripAmount(uint256 _ethAmount) public {
        FAUCET.updateDripAmount(_ethAmount);
    }

    /// @notice Updates developer drip amount
    /// @param _ethAmount ETH to drip to developers
    function updateDeveloperDripAmount(uint256 _ethAmount) public {
        FAUCET.updateDeveloperDripAmount(_ethAmount);
    }

    /// @notice Updates whitelist drip amount
    /// @param _ethAmount ETH to drip to whitelisted users
    function updateWhitelistDripAmount(uint256 _ethAmount) public {
        FAUCET.updateWhitelistDripAmount(_ethAmount);
    }

    /// @notice Allows receiving ETH
    receive() external payable {}
}
