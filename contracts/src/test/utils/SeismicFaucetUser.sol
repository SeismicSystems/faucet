// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.30;

/// ============ Imports ============

import "../../SeismicFaucet.sol";

/// @title SeismicFaucetUser
/// @author Ameya Deshmukh
/// @dev Based on Anish Agnihotri's MultiFaucetUser (https://github.com/Anish-Agnihotri/MultiFaucet)
/// @notice Mock user to test interacting with SeismicFaucet
contract SeismicFaucetUser {
    SeismicFaucet internal immutable FAUCET;

    constructor(SeismicFaucet _FAUCET) {
        FAUCET = _FAUCET;
    }

    /// @notice Returns this user's SUSDC balance (SRC20.balance() is keyed on msg.sender)
    function SUSDCBalance() public view returns (uint256) {
        return FAUCET.susdc().balance();
    }

    function drip(address _recipient) public {
        FAUCET.drip(_recipient);
    }

    function dripDeveloper(address _recipient) public {
        FAUCET.dripDeveloper(_recipient);
    }

    function dripWhitelist(address _recipient) public {
        FAUCET.dripWhitelist(_recipient);
    }

    function drain(address _recipient) public {
        FAUCET.drain(_recipient);
    }

    function updateApprovedOperator(address _operator, bool _status) public {
        FAUCET.updateApprovedOperator(_operator, _status);
    }

    function updateSuperOperator(address _operator, bool _status) public {
        FAUCET.updateSuperOperator(_operator, _status);
    }

    function updateDripAmount(uint256 _amount) public {
        FAUCET.updateDripAmount(_amount);
    }

    function updateDeveloperDripAmount(uint256 _amount) public {
        FAUCET.updateDeveloperDripAmount(_amount);
    }

    function updateWhitelistDripAmount(uint256 _amount) public {
        FAUCET.updateWhitelistDripAmount(_amount);
    }
}
