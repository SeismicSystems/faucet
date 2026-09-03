use alloy_primitives::{Address, U256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FundingAsset {
    Susdc,
    SusdcGas,
    Erc20Usdc,
    BaseEth,
    BaseErc20Usdc,
}

impl FundingAsset {
    pub fn key(self) -> &'static str {
        match self {
            Self::Susdc => "susdc",
            Self::SusdcGas => "susdc_gas",
            Self::Erc20Usdc => "erc20_usdc",
            Self::BaseEth => "base_eth",
            Self::BaseErc20Usdc => "base_erc20_usdc",
        }
    }

    pub fn is_base(self) -> bool {
        matches!(self, Self::BaseEth | Self::BaseErc20Usdc)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingInput {
    pub asset: FundingAsset,
    pub deployment_identity: Option<Erc20DeploymentIdentity>,
    pub network_identity: Option<BaseNetworkIdentity>,
    pub idempotency_key: String,
    pub recipient: Address,
    pub recipient_text: String,
    pub amount: U256,
    pub reason: String,
}

/// The Base deployment a request is bound to. Every idempotency record,
/// budget, and rate key on Base carries this scope, so a token redeploy or a
/// reserve-key rotation starts a fresh ledger instead of replaying the old one.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BaseNetworkIdentity {
    pub chain_id: u64,
    pub token_address: String,
    pub reserve_address: String,
}

impl BaseNetworkIdentity {
    pub fn scope(&self) -> String {
        format!(
            "base:{}:{}:{}",
            self.chain_id,
            self.token_address.to_ascii_lowercase(),
            self.reserve_address.to_ascii_lowercase()
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Erc20DeploymentIdentity {
    pub chain_id: u64,
    pub token_address: String,
    pub faucet_address: String,
    pub operator_address: String,
    pub reserve_address: String,
}

impl Erc20DeploymentIdentity {
    pub fn scope(&self) -> String {
        format!(
            "erc20_usdc:{}:{}:{}:{}:{}",
            self.chain_id,
            self.token_address.to_ascii_lowercase(),
            self.faucet_address.to_ascii_lowercase(),
            self.operator_address.to_ascii_lowercase(),
            self.reserve_address.to_ascii_lowercase()
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PersistedInput {
    pub asset: FundingAsset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_identity: Option<Erc20DeploymentIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_identity: Option<BaseNetworkIdentity>,
    pub idempotency_key: String,
    pub recipient: String,
    pub amount: String,
    pub reason: String,
}

impl From<&FundingInput> for PersistedInput {
    fn from(input: &FundingInput) -> Self {
        Self {
            asset: input.asset,
            deployment_identity: input.deployment_identity.clone(),
            network_identity: input.network_identity.clone(),
            idempotency_key: input.idempotency_key.clone(),
            recipient: input.recipient_text.clone(),
            amount: input.amount.to_string(),
            reason: input.reason.clone(),
        }
    }
}

impl TryFrom<&PersistedInput> for FundingInput {
    type Error = ServiceError;

    fn try_from(input: &PersistedInput) -> Result<Self, Self::Error> {
        let recipient = input
            .recipient
            .parse()
            .map_err(|_| ServiceError::internal())?;
        let amount =
            U256::from_str_radix(&input.amount, 10).map_err(|_| ServiceError::internal())?;
        Ok(Self {
            asset: input.asset,
            deployment_identity: input.deployment_identity.clone(),
            network_identity: input.network_identity.clone(),
            idempotency_key: input.idempotency_key.clone(),
            recipient,
            recipient_text: input.recipient.clone(),
            amount,
            reason: input.reason.clone(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PreparedTransaction {
    pub hash: String,
    pub nonce: u64,
    pub serialized_transaction: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FundingResponse {
    pub idempotency_key: String,
    pub recipient: String,
    pub amount: String,
    pub transaction_hash: String,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum FundingRecord {
    Queued {
        fingerprint: String,
        input: PersistedInput,
    },
    Prepared {
        fingerprint: String,
        input: PersistedInput,
        transaction: PreparedTransaction,
    },
    Completed {
        fingerprint: String,
        input: PersistedInput,
        transaction: PreparedTransaction,
        response: FundingResponse,
    },
    Failed {
        fingerprint: String,
        input: PersistedInput,
        transaction: PreparedTransaction,
        code: String,
        message: String,
        transaction_hash: String,
    },
    Rejected {
        fingerprint: String,
        input: PersistedInput,
        code: String,
        message: String,
    },
}

impl FundingRecord {
    pub fn fingerprint(&self) -> &str {
        match self {
            Self::Queued { fingerprint, .. }
            | Self::Prepared { fingerprint, .. }
            | Self::Completed { fingerprint, .. }
            | Self::Failed { fingerprint, .. }
            | Self::Rejected { fingerprint, .. } => fingerprint,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SusdcRequest {
    pub idempotency_key: String,
    pub recipient: String,
    pub amount: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GasRequest {
    pub idempotency_key: String,
    pub recipient: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Clone, Debug, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_hash: Option<String>,
}

#[derive(Clone, Debug, Error)]
#[error("{message}")]
pub struct ServiceError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub retry_after_seconds: Option<u64>,
    pub transaction_hash: Option<String>,
}

impl ServiceError {
    pub fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            retry_after_seconds: None,
            transaction_hash: None,
        }
    }

    pub fn retry(mut self, seconds: u64) -> Self {
        self.retry_after_seconds = Some(seconds);
        self
    }

    pub fn transaction(mut self, hash: impl Into<String>) -> Self {
        self.transaction_hash = Some(hash.into());
        self
    }

    pub fn internal() -> Self {
        Self::new(500, "internal_error", "Machine funding state is invalid")
    }
}

/// Reserve health of a funding signer, for operators: balances alongside
/// the configured floors and whether either has been breached.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReserveDiagnostic {
    pub chain_id: u64,
    pub reserve_address: String,
    pub native_balance: String,
    pub native_floor: String,
    pub native_low: bool,
    pub erc20_usdc_balance: String,
    pub erc20_usdc_floor: String,
    pub erc20_usdc_low: bool,
}

impl ReserveDiagnostic {
    pub fn is_low(&self) -> bool {
        self.native_low || self.erc20_usdc_low
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainResult {
    Success,
    Reverted,
    Pending,
    Rejected(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReservationResult {
    Reserved,
    Existing,
    RecipientRateLimited(u64),
    GlobalBudgetExceeded(u64),
}
