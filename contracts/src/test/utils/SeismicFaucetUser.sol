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

    /// @notice Returns this user's SUSDC balance
    /// @dev SRC20.balance() reads `balances[msg.sender]`, so calling from
    ///      within this contract returns this contract's own balance.
    function SUSDCBalance() public view returns (uint256) {
        return FAUCET.susdc().balance();
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

    /// @notice Transfers an exact SUSDC amount from the faucet
    /// @param _recipient recipient address
    /// @param _amount SUSDC amount in 6-decimal base units
    function transferExact(address _recipient, uint256 _amount) public {
        FAUCET.transferExact(_recipient, _amount);
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

    /// @notice Adds or removes an exact-transfer operator
    /// @param _operator address
    /// @param _status whether the operator may submit exact transfers
    function updateMachineOperator(address _operator, bool _status) public {
        FAUCET.updateMachineOperator(_operator, _status);
    }

    /// @notice Updates super operator
    /// @param _operator address
    /// @param _status of operator
    function updateSuperOperator(address _operator, bool _status) public {
        FAUCET.updateSuperOperator(_operator, _status);
    }

    /// @notice Updates drip amount
    /// @param _amount SUSDC to drip (6 decimals)
    function updateDripAmount(uint256 _amount) public {
        FAUCET.updateDripAmount(_amount);
    }

    /// @notice Updates developer drip amount
    /// @param _amount SUSDC to drip to developers (6 decimals)
    function updateDeveloperDripAmount(uint256 _amount) public {
        FAUCET.updateDeveloperDripAmount(_amount);
    }

    /// @notice Updates whitelist drip amount
    /// @param _amount SUSDC to drip to whitelisted users (6 decimals)
    function updateWhitelistDripAmount(uint256 _amount) public {
        FAUCET.updateWhitelistDripAmount(_amount);
    }

    /// @notice Updates the exact transfer ceiling
    /// @param _amount maximum SUSDC per transfer in 6-decimal base units
    function updateMaxExactTransferAmount(uint256 _amount) public {
        FAUCET.updateMaxExactTransferAmount(_amount);
    }
}
