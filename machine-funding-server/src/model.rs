use alloy_primitives::{Address, U256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FundingAsset {
    Susdc,
    SusdcGas,
}

impl FundingAsset {
    pub fn key(self) -> &'static str {
        match self {
            Self::Susdc => "susdc",
            Self::SusdcGas => "susdc_gas",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingInput {
    pub asset: FundingAsset,
    pub idempotency_key: String,
    pub recipient: Address,
    pub recipient_text: String,
    pub amount: U256,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PersistedInput {
    pub asset: FundingAsset,
    pub idempotency_key: String,
    pub recipient: String,
    pub amount: String,
    pub reason: String,
}

impl From<&FundingInput> for PersistedInput {
    fn from(input: &FundingInput) -> Self {
        Self {
            asset: input.asset,
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
}

impl FundingRecord {
    pub fn fingerprint(&self) -> &str {
        match self {
            Self::Queued { fingerprint, .. }
            | Self::Prepared { fingerprint, .. }
            | Self::Completed { fingerprint, .. }
            | Self::Failed { fingerprint, .. } => fingerprint,
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
