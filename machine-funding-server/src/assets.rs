use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{sol, SolCall};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use thiserror::Error;

const MAX_DECIMALS: u8 = 18;
const MAX_MINT_CALLS: u16 = 32;
const MAX_LEDGER_AMOUNT: u64 = i64::MAX as u64;

sol! {
    function mint(uint256 amount);
}

/// Funding capabilities are operator-reviewed; a mint selector alone does not
/// establish permissionless access or the argument's units.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FundingStrategy {
    ManualInventory,
    CallerMintWholeTokens {
        max_tokens_per_call: u64,
        max_calls: u16,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FundingToken {
    pub contract_address: Address,
    pub decimals: u8,
    #[serde(deserialize_with = "decimal_amount")]
    pub max_transfer: U256,
    #[serde(deserialize_with = "decimal_amount")]
    pub global_budget: U256,
    #[serde(deserialize_with = "decimal_amount")]
    pub reserve_floor: U256,
    pub funding: FundingStrategy,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AssetError {
    #[error("invalid token address or decimals")]
    InvalidIdentity,
    #[error("token limits must fit the ledger and budget must cover a transfer")]
    InvalidLimits,
    #[error("minting must have positive, bounded per-call and per-request limits")]
    InvalidMintLimits,
    #[error("duplicate token contract")]
    DuplicateContract,
    #[error("manual reserve funding is required")]
    ManualFundingRequired,
    #[error("replenishment exceeds the configured mint call limit")]
    MintLimitExceeded,
    #[error("requested transfer exceeds the configured token limit")]
    TransferLimitExceeded,
}

/// A plan only: the executor must persist and confirm each mint separately
/// from the payout before retrying either operation.
#[derive(Debug, PartialEq, Eq)]
pub struct ReplenishmentPlan {
    pub calldata: Vec<Bytes>,
    pub minted_base_units: U256,
}

impl FundingToken {
    pub fn validate(&self) -> Result<(), AssetError> {
        if self.contract_address == Address::ZERO || self.decimals > MAX_DECIMALS {
            return Err(AssetError::InvalidIdentity);
        }
        let cap = U256::from(MAX_LEDGER_AMOUNT);
        if self.max_transfer == U256::ZERO
            || self.global_budget < self.max_transfer
            || self.global_budget > cap
            || self.reserve_floor > cap
        {
            return Err(AssetError::InvalidLimits);
        }
        if let FundingStrategy::CallerMintWholeTokens {
            max_tokens_per_call,
            max_calls,
        } = self.funding
        {
            if max_tokens_per_call == 0 || max_calls == 0 || max_calls > MAX_MINT_CALLS {
                return Err(AssetError::InvalidMintLimits);
            }
        }
        Ok(())
    }

    pub fn replenishment(
        &self,
        balance: U256,
        transfer: U256,
    ) -> Result<ReplenishmentPlan, AssetError> {
        self.validate()?;
        if transfer == U256::ZERO || transfer > self.max_transfer {
            return Err(AssetError::TransferLimitExceeded);
        }
        let target = transfer.max(self.reserve_floor);
        if balance >= target {
            return Ok(ReplenishmentPlan {
                calldata: Vec::new(),
                minted_base_units: U256::ZERO,
            });
        }
        let FundingStrategy::CallerMintWholeTokens {
            max_tokens_per_call,
            max_calls,
        } = self.funding
        else {
            return Err(AssetError::ManualFundingRequired);
        };
        let scale = U256::from(10).pow(U256::from(self.decimals));
        let deficit = target - balance;
        let whole_tokens = (deficit + scale - U256::from(1)) / scale;
        let per_call = U256::from(max_tokens_per_call);
        let count = (whole_tokens + per_call - U256::from(1)) / per_call;
        if count > U256::from(max_calls) {
            return Err(AssetError::MintLimitExceeded);
        }
        let mut remaining = whole_tokens;
        let mut calldata = Vec::new();
        while remaining > U256::ZERO {
            let amount = remaining.min(per_call);
            calldata.push(mintCall { amount }.abi_encode().into());
            remaining -= amount;
        }
        Ok(ReplenishmentPlan {
            calldata,
            minted_base_units: whole_tokens * scale,
        })
    }
}

#[derive(Debug)]
pub struct FundingCatalog {
    default: Address,
    tokens: BTreeMap<Address, FundingToken>,
}

impl FundingCatalog {
    pub fn new(
        default: FundingToken,
        additional: impl IntoIterator<Item = FundingToken>,
    ) -> Result<Self, AssetError> {
        let default_address = default.contract_address;
        let mut tokens = BTreeMap::new();
        for token in std::iter::once(default).chain(additional) {
            token.validate()?;
            if tokens.insert(token.contract_address, token).is_some() {
                return Err(AssetError::DuplicateContract);
            }
        }
        Ok(Self {
            default: default_address,
            tokens,
        })
    }

    pub fn resolve(&self, contract: Option<Address>) -> Option<&FundingToken> {
        self.tokens.get(&contract.unwrap_or(self.default))
    }
}

fn decimal_amount<'de, D: Deserializer<'de>>(deserializer: D) -> Result<U256, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(serde::de::Error::custom(
            "expected decimal base-unit string",
        ));
    }
    U256::from_str_radix(&value, 10).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rusd() -> FundingToken {
        FundingToken {
            contract_address: "0x10b5Be494C2962A7B318aFB63f0Ee30b959D000b"
                .parse()
                .unwrap(),
            decimals: 6,
            max_transfer: U256::from(500_000_000),
            global_budget: U256::from(1_000_000_000),
            reserve_floor: U256::ZERO,
            funding: FundingStrategy::CallerMintWholeTokens {
                max_tokens_per_call: 100,
                max_calls: 4,
            },
        }
    }

    #[test]
    fn rusd_uses_whole_tokens_and_bounded_chunks() {
        let token = rusd();
        let plan = token
            .replenishment(U256::ZERO, U256::from(250_000_001))
            .unwrap();
        let amounts: Vec<_> = plan
            .calldata
            .iter()
            .map(|data| mintCall::abi_decode(data).unwrap().amount)
            .collect();
        assert_eq!(
            amounts,
            vec![U256::from(100), U256::from(100), U256::from(51)]
        );
        assert_eq!(plan.minted_base_units, U256::from(251_000_000));
        assert_eq!(
            token.replenishment(U256::ZERO, U256::from(400_000_001)),
            Err(AssetError::MintLimitExceeded)
        );
    }

    #[test]
    fn manual_inventory_can_pay_but_cannot_replenish() {
        let mut token = rusd();
        token.funding = FundingStrategy::ManualInventory;
        assert!(token
            .replenishment(U256::from(10), U256::from(10))
            .unwrap()
            .calldata
            .is_empty());
        assert_eq!(
            token.replenishment(U256::from(9), U256::from(10)),
            Err(AssetError::ManualFundingRequired)
        );
    }

    #[test]
    fn replenishment_accounts_for_existing_balance_and_floor() {
        let mut token = rusd();
        token.reserve_floor = U256::from(100_000_000);
        let plan = token
            .replenishment(U256::from(99_999_999), U256::from(1))
            .unwrap();
        assert_eq!(plan.minted_base_units, U256::from(1_000_000));
        assert_eq!(
            mintCall::abi_decode(&plan.calldata[0]).unwrap().amount,
            U256::from(1)
        );
        assert!(token
            .replenishment(U256::from(100_000_000), U256::from(1))
            .unwrap()
            .calldata
            .is_empty());
    }

    #[test]
    fn catalog_defaults_are_stable_and_unknown_contracts_do_not_fallback() {
        let default = rusd();
        let mut other = default.clone();
        other.contract_address = Address::with_last_byte(1);
        other.funding = FundingStrategy::ManualInventory;
        let catalog = FundingCatalog::new(default.clone(), [other.clone()]).unwrap();
        assert_eq!(catalog.resolve(None), Some(&default));
        assert_eq!(catalog.resolve(Some(other.contract_address)), Some(&other));
        assert!(catalog.resolve(Some(Address::with_last_byte(2))).is_none());
        assert!(matches!(
            FundingCatalog::new(default.clone(), [default]),
            Err(AssetError::DuplicateContract)
        ));
    }

    #[test]
    fn unsafe_limits_and_ambiguous_amounts_are_rejected() {
        let mut token = rusd();
        token.decimals = 19;
        assert_eq!(token.validate(), Err(AssetError::InvalidIdentity));
        token = rusd();
        token.global_budget = U256::from(1);
        assert_eq!(token.validate(), Err(AssetError::InvalidLimits));
        token = rusd();
        token.funding = FundingStrategy::CallerMintWholeTokens {
            max_tokens_per_call: 100,
            max_calls: 33,
        };
        assert_eq!(token.validate(), Err(AssetError::InvalidMintLimits));
        let json = r#"{"contract_address":"0x10b5be494c2962a7b318afb63f0ee30b959d000b","decimals":6,"max_transfer":"100","global_budget":"200","reserve_floor":"0","funding":{"kind":"manual_inventory"}}"#;
        assert_eq!(
            serde_json::from_str::<FundingToken>(json)
                .unwrap()
                .max_transfer,
            U256::from(100)
        );
        assert!(
            serde_json::from_str::<FundingToken>(&json.replace("\"100\"", "\"0x64\"")).is_err()
        );
        assert!(serde_json::from_str::<FundingToken>(
            &json.replace("manual_inventory", "auto_detect")
        )
        .is_err());
    }
}
