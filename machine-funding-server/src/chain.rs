use crate::{
    config::{Config, Erc20UsdcConfig},
    model::{
        ChainResult, FundingAsset, FundingInput, PreparedTransaction, ReserveDiagnostic,
        ServiceError,
    },
};
use alloy_eips::eip2718::Encodable2718;
use alloy_network::TransactionBuilder;
use alloy_primitives::{keccak256, Address, Bytes, TxKind, B256, U256};
use alloy_provider::Provider;
use alloy_rpc_types_eth::{TransactionInput, TransactionRequest};
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{sol, SolCall};
use async_trait::async_trait;
use seismic_alloy_network::{wallet::SeismicWallet, SeismicReth};
use seismic_alloy_provider::{SeismicProviderBuilder, SeismicUnsignedProvider};
use seismic_alloy_rpc_types::{SeismicTransactionReceipt, SeismicTransactionRequest};
use std::{str::FromStr, time::Duration};
use tokio::time::{sleep, timeout};
use url::Url;

const LEGACY_TRANSACTION_TYPE: u8 = 0;
const TRANSACTION_GAS_LIMIT: u64 = 500_000;
const GAS_PRICE_MULTIPLIER: u128 = 2;
const RECEIPT_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub(crate) const RPC_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const TERMINAL_BROADCAST_ERRORS: &[&str] = &[
    "insufficient funds",
    "intrinsic gas too low",
    "invalid sender",
    "invalid chain id",
    "transaction type not supported",
];

sol! {
    #[sol(rpc)]
    interface SeismicFaucet {
        function transferExact(address _recipient, uint256 _amount) external;
        function machineOperators(address operator) external view returns (bool);
        function superOperators(address operator) external view returns (bool);
        function MAX_EXACT_TRANSFER_AMOUNT() external view returns (uint256);
    }

    #[sol(rpc)]
    interface ERC20USDCFaucet {
        function transferExact(address recipient, uint256 amount) external;
        function machineOperators(address operator) external view returns (bool);
        function superOperators(address operator) external view returns (bool);
        function MAX_EXACT_TRANSFER_AMOUNT() external view returns (uint256);
        function usdc() external view returns (address);
    }

    #[sol(rpc)]
    interface ERC20USDC {
        function decimals() external view returns (uint8);
        function balanceOf(address account) external view returns (uint256);
        function owner() external view returns (address);
    }
}

#[async_trait]
pub trait ChainDriver: Clone + Send + Sync + 'static {
    fn operator_key(&self) -> &str;
    fn validate_input(&self, _input: &FundingInput) -> Result<(), ServiceError> {
        Ok(())
    }
    async fn pending_nonce(&self) -> Result<u64, ServiceError>;
    async fn can_retry_reverted(
        &self,
        _input: &FundingInput,
        _transaction: &PreparedTransaction,
    ) -> Result<bool, ServiceError> {
        Ok(false)
    }
    async fn prepare(
        &self,
        input: &FundingInput,
        nonce: u64,
    ) -> Result<PreparedTransaction, ServiceError>;
    async fn broadcast_and_confirm(
        &self,
        transaction: &PreparedTransaction,
    ) -> Result<ChainResult, ServiceError>;
    async fn health(&self) -> Result<(), ServiceError>;
    /// Reserve balances for operators; `None` for drivers whose reserves are
    /// held by a contract rather than the signer.
    async fn reserve_diagnostic(&self) -> Result<Option<ReserveDiagnostic>, ServiceError> {
        Ok(None)
    }
}

#[derive(Clone)]
pub struct EvmChainDriver {
    provider: SeismicUnsignedProvider<SeismicReth>,
    wallet: SeismicWallet<SeismicReth>,
    operator_address: Address,
    faucet_address: Address,
    chain_id: u64,
    operator_key: String,
    confirmations: u64,
    receipt_timeout: Duration,
    erc20_usdc: Option<Erc20UsdcConfig>,
}

impl EvmChainDriver {
    pub async fn connect(config: &Config) -> Result<Self, ServiceError> {
        let rpc_url = Url::parse(&config.rpc_url).map_err(chain_error)?;
        let provider = SeismicProviderBuilder::new().connect_http(rpc_url);
        let chain_id = rpc_timeout(provider.get_chain_id()).await?;
        if chain_id != config.chain_id {
            return Err(chain_error(format!(
                "RPC chain id {chain_id} does not match configured chain id {}",
                config.chain_id
            )));
        }
        let signer = PrivateKeySigner::from_str(&config.private_key).map_err(chain_error)?;
        let operator_address = signer.address();
        if operator_address != config.funding_address {
            return Err(chain_error(
                "INTERNAL_FUNDING_PRIVATE_KEY does not match INTERNAL_FUNDING_ADDRESS",
            ));
        }
        let operator_key = format!("{chain_id}:{operator_address:#x}");
        let driver = Self {
            provider,
            wallet: SeismicWallet::from(signer),
            operator_address,
            faucet_address: config.faucet_address,
            chain_id,
            operator_key,
            confirmations: config.confirmations,
            receipt_timeout: config.receipt_timeout,
            erc20_usdc: None,
        };
        driver
            .validate_faucet(std::cmp::max(
                config.max_susdc_amount,
                config.gas_susdc_amount,
            ))
            .await?;
        Ok(driver)
    }

    pub async fn connect_erc20_usdc(
        config: &Config,
        erc20_usdc: &Erc20UsdcConfig,
    ) -> Result<Self, ServiceError> {
        let rpc_url = Url::parse(&config.rpc_url).map_err(chain_error)?;
        let provider = SeismicProviderBuilder::new().connect_http(rpc_url);
        let chain_id = rpc_timeout(provider.get_chain_id()).await?;
        if chain_id != config.chain_id {
            return Err(chain_error(format!(
                "RPC chain id {chain_id} does not match configured chain id {}",
                config.chain_id
            )));
        }
        let signer = PrivateKeySigner::from_str(&erc20_usdc.private_key).map_err(chain_error)?;
        let operator_address = signer.address();
        if operator_address != erc20_usdc.funding_address {
            return Err(chain_error(
                "INTERNAL_FUNDING_ERC20_USDC_PRIVATE_KEY does not match INTERNAL_FUNDING_ERC20_USDC_ADDRESS",
            ));
        }
        let operator_key = format!("{chain_id}:{operator_address:#x}");
        let driver = Self {
            provider,
            wallet: SeismicWallet::from(signer),
            operator_address,
            faucet_address: config.faucet_address,
            chain_id,
            operator_key,
            confirmations: config.confirmations,
            receipt_timeout: config.receipt_timeout,
            erc20_usdc: Some(erc20_usdc.clone()),
        };
        driver.validate_erc20_usdc(erc20_usdc).await?;
        Ok(driver)
    }

    fn transfer_exact_data(input: &FundingInput) -> Bytes {
        match input.asset {
            FundingAsset::Susdc | FundingAsset::SusdcGas => SeismicFaucet::transferExactCall {
                _recipient: input.recipient,
                _amount: input.amount,
            }
            .abi_encode()
            .into(),
            FundingAsset::Erc20Usdc => ERC20USDCFaucet::transferExactCall {
                recipient: input.recipient,
                amount: input.amount,
            }
            .abi_encode()
            .into(),
            FundingAsset::BaseEth | FundingAsset::BaseErc20Usdc => Bytes::new(),
        }
    }

    fn transfer_target(&self, asset: FundingAsset) -> Result<Address, ServiceError> {
        match asset {
            FundingAsset::Susdc | FundingAsset::SusdcGas => Ok(self.faucet_address),
            FundingAsset::Erc20Usdc => self
                .erc20_usdc
                .as_ref()
                .map(|config| config.faucet_address)
                .ok_or_else(deployment_identity_error),
            FundingAsset::BaseEth | FundingAsset::BaseErc20Usdc => Err(deployment_identity_error()),
        }
    }

    fn transaction_request(
        &self,
        input: &FundingInput,
        nonce: u64,
        gas_price: u128,
    ) -> Result<SeismicTransactionRequest, ServiceError> {
        Ok(SeismicTransactionRequest {
            inner: TransactionRequest {
                from: Some(self.operator_address),
                to: Some(TxKind::Call(self.transfer_target(input.asset)?)),
                gas_price: Some(gas_price),
                gas: Some(TRANSACTION_GAS_LIMIT),
                value: Some(U256::ZERO),
                input: TransactionInput::from(Self::transfer_exact_data(input)),
                nonce: Some(nonce),
                chain_id: Some(self.chain_id),
                transaction_type: Some(LEGACY_TRANSACTION_TYPE),
                ..Default::default()
            },
            seismic_elements: None,
        })
    }

    async fn receipt_result(&self, hash: B256) -> Result<ChainResult, ServiceError> {
        match timeout(self.receipt_timeout, async {
            loop {
                let receipt = rpc_timeout(self.provider.get_transaction_receipt(hash)).await?;
                match receipt_status(receipt.as_ref()) {
                    Some(ChainResult::Reverted) => return Ok(ChainResult::Reverted),
                    Some(ChainResult::Success) => {
                        let Some(receipt_block) = receipt
                            .as_ref()
                            .and_then(|confirmed| confirmed.block_number)
                        else {
                            return Err(ServiceError::internal());
                        };
                        let latest_block = rpc_timeout(self.provider.get_block_number()).await?;
                        let required_block =
                            receipt_block.saturating_add(self.confirmations.saturating_sub(1));
                        if latest_block >= required_block {
                            return Ok(ChainResult::Success);
                        }
                    }
                    _ => {}
                }
                sleep(RECEIPT_POLL_INTERVAL).await;
            }
        })
        .await
        {
            Ok(result) => result,
            Err(_) => Ok(ChainResult::Pending),
        }
    }

    async fn validate_faucet(&self, configured_max: U256) -> Result<(), ServiceError> {
        let code =
            rpc_timeout(async { self.provider.get_code_at(self.faucet_address).await }).await?;
        if code.is_empty() {
            return Err(chain_error("FAUCET_ADDRESS has no deployed bytecode"));
        }
        let faucet = SeismicFaucet::new(self.faucet_address, &self.provider);
        let machine_operator =
            rpc_timeout(async { faucet.machineOperators(self.operator_address).call().await })
                .await?;
        if !machine_operator {
            return Err(chain_error(
                "internal funding signer is not a machine operator",
            ));
        }
        let super_operator =
            rpc_timeout(async { faucet.superOperators(self.operator_address).call().await })
                .await?;
        if super_operator {
            return Err(chain_error(
                "internal funding signer must not be a super operator",
            ));
        }
        let on_chain_max =
            rpc_timeout(async { faucet.MAX_EXACT_TRANSFER_AMOUNT().call().await }).await?;
        if configured_max > on_chain_max {
            return Err(chain_error(format!(
                "configured SUSDC maximum {configured_max} exceeds contract maximum {on_chain_max}"
            )));
        }
        Ok(())
    }

    async fn validate_erc20_usdc(&self, config: &Erc20UsdcConfig) -> Result<(), ServiceError> {
        let token_code =
            rpc_timeout(async { self.provider.get_code_at(config.token_address).await }).await?;
        if token_code.is_empty() {
            return Err(chain_error(
                "ERC20_USDC_TOKEN_ADDRESS has no deployed bytecode",
            ));
        }
        let faucet_code =
            rpc_timeout(async { self.provider.get_code_at(config.faucet_address).await }).await?;
        if faucet_code.is_empty() {
            return Err(chain_error(
                "ERC20_USDC_FAUCET_ADDRESS has no deployed bytecode",
            ));
        }

        let faucet = ERC20USDCFaucet::new(config.faucet_address, &self.provider);
        let configured_token = rpc_timeout(async { faucet.usdc().call().await }).await?;
        if configured_token != config.token_address {
            return Err(chain_error(
                "ERC20 USDC faucet token does not match ERC20_USDC_TOKEN_ADDRESS",
            ));
        }
        let machine_operator =
            rpc_timeout(async { faucet.machineOperators(self.operator_address).call().await })
                .await?;
        if !machine_operator {
            return Err(chain_error(
                "internal funding signer is not an ERC20 USDC machine operator",
            ));
        }
        let super_operator =
            rpc_timeout(async { faucet.superOperators(self.operator_address).call().await })
                .await?;
        if super_operator {
            return Err(chain_error(
                "internal funding signer must not be an ERC20 USDC super operator",
            ));
        }
        let reserve_super_operator =
            rpc_timeout(async { faucet.superOperators(config.reserve_address).call().await })
                .await?;
        if !reserve_super_operator {
            return Err(chain_error(
                "ERC20 USDC reserve is not a faucet super operator",
            ));
        }
        let on_chain_max =
            rpc_timeout(async { faucet.MAX_EXACT_TRANSFER_AMOUNT().call().await }).await?;
        if config.max_amount > on_chain_max {
            return Err(chain_error(format!(
                "configured ERC20 USDC maximum {} exceeds contract maximum {on_chain_max}",
                config.max_amount
            )));
        }

        let token = ERC20USDC::new(config.token_address, &self.provider);
        let token_owner = rpc_timeout(async { token.owner().call().await }).await?;
        if token_owner != config.reserve_address {
            return Err(chain_error(
                "ERC20 USDC reserve does not own the token contract",
            ));
        }
        let decimals = rpc_timeout(async { token.decimals().call().await }).await?;
        if decimals != 6 {
            return Err(chain_error(format!(
                "ERC20 USDC token has {decimals} decimals; expected 6"
            )));
        }
        let faucet_balance =
            rpc_timeout(async { token.balanceOf(config.faucet_address).call().await }).await?;
        if faucet_balance < config.max_amount {
            return Err(chain_error(format!(
                "ERC20 USDC faucet balance {faucet_balance} is below configured maximum {}",
                config.max_amount
            )));
        }
        Ok(())
    }

    async fn sign_transaction(
        &self,
        input: &FundingInput,
        nonce: u64,
        gas_price: u128,
    ) -> Result<PreparedTransaction, ServiceError> {
        let request = self.transaction_request(input, nonce, gas_price)?;
        let envelope = TransactionBuilder::<SeismicReth>::build(request, &self.wallet)
            .await
            .map_err(chain_error)?;
        let serialized = envelope.encoded_2718();
        let hash = keccak256(&serialized);
        Ok(PreparedTransaction {
            hash: format!("{hash:#x}"),
            nonce,
            serialized_transaction: format!("0x{}", hex::encode(serialized)),
        })
    }
}

#[async_trait]
impl ChainDriver for EvmChainDriver {
    fn operator_key(&self) -> &str {
        &self.operator_key
    }

    fn validate_input(&self, input: &FundingInput) -> Result<(), ServiceError> {
        match (input.asset, &self.erc20_usdc) {
            (FundingAsset::Erc20Usdc, Some(config))
                if input.deployment_identity.as_ref() == Some(&config.identity(self.chain_id)) =>
            {
                Ok(())
            }
            (FundingAsset::Susdc | FundingAsset::SusdcGas, None)
                if input.deployment_identity.is_none() && input.network_identity.is_none() =>
            {
                Ok(())
            }
            _ => Err(deployment_identity_error()),
        }
    }

    async fn pending_nonce(&self) -> Result<u64, ServiceError> {
        rpc_timeout(async {
            self.provider
                .get_transaction_count(self.operator_address)
                .pending()
                .await
        })
        .await
    }

    async fn prepare(
        &self,
        input: &FundingInput,
        nonce: u64,
    ) -> Result<PreparedTransaction, ServiceError> {
        let quoted_gas_price = rpc_timeout(self.provider.get_gas_price()).await?;
        let gas_price = quoted_gas_price
            .checked_mul(GAS_PRICE_MULTIPLIER)
            .ok_or_else(|| chain_error("RPC gas price overflow"))?;
        self.sign_transaction(input, nonce, gas_price).await
    }

    async fn broadcast_and_confirm(
        &self,
        transaction: &PreparedTransaction,
    ) -> Result<ChainResult, ServiceError> {
        let raw = hex::decode(transaction.serialized_transaction.trim_start_matches("0x"))
            .map_err(chain_error)?;
        let expected_hash = B256::from_str(&transaction.hash).map_err(chain_error)?;
        match timeout(
            RPC_REQUEST_TIMEOUT,
            self.provider.send_raw_transaction(&raw),
        )
        .await
        {
            Ok(Ok(pending)) if *pending.tx_hash() != expected_hash => {
                return Ok(ChainResult::Rejected(
                    "RPC returned a different transaction hash".into(),
                ));
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                let message = error.to_string();
                if let Some(result) = classify_broadcast_error(&message) {
                    return Ok(result);
                }
                tracing::warn!(%message, hash = %transaction.hash, "raw transaction broadcast was not acknowledged");
            }
            Err(_) => {
                tracing::warn!(hash = %transaction.hash, "raw transaction broadcast timed out");
            }
        }
        self.receipt_result(expected_hash).await
    }

    async fn health(&self) -> Result<(), ServiceError> {
        let Some(erc20_usdc) = &self.erc20_usdc else {
            return rpc_timeout(self.provider.get_chain_id()).await.map(|_| ());
        };
        let chain_id = rpc_timeout(self.provider.get_chain_id()).await?;
        if chain_id != self.chain_id {
            return Err(chain_error(format!(
                "RPC chain id changed from {} to {chain_id}",
                self.chain_id
            )));
        }
        self.validate_erc20_usdc(erc20_usdc).await?;
        Ok(())
    }
}

pub(crate) fn classify_broadcast_error(message: &str) -> Option<ChainResult> {
    let lower = message.to_ascii_lowercase();
    TERMINAL_BROADCAST_ERRORS
        .iter()
        .any(|candidate| lower.contains(candidate))
        .then(|| ChainResult::Rejected(message.to_owned()))
}

fn receipt_status(receipt: Option<&SeismicTransactionReceipt>) -> Option<ChainResult> {
    let receipt = receipt?;
    receipt.block_number?;
    receipt.block_hash?;
    if receipt.inner.status() {
        Some(ChainResult::Success)
    } else {
        Some(ChainResult::Reverted)
    }
}

pub(crate) async fn rpc_timeout<T, E>(
    future: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, ServiceError>
where
    E: std::fmt::Display,
{
    timeout(RPC_REQUEST_TIMEOUT, future)
        .await
        .map_err(|_| chain_error("RPC request timed out"))?
        .map_err(chain_error)
}

pub(crate) fn chain_error(error: impl std::fmt::Display) -> ServiceError {
    tracing::error!(%error, "machine funding chain operation failed");
    ServiceError::new(
        502,
        "transaction_failed",
        "Unable to submit funding transaction",
    )
}

pub(crate) fn deployment_identity_error() -> ServiceError {
    ServiceError::new(
        409,
        "deployment_identity_mismatch",
        "Funding request belongs to a different contract deployment",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FundingAsset, FundingInput};
    use alloy_sol_types::SolCall;

    const TRANSFER_EXACT_SELECTOR: [u8; 4] = [0xe1, 0x72, 0xa7, 0xc3];

    fn driver() -> EvmChainDriver {
        let signer = PrivateKeySigner::from_str(&format!("0x{}", "1".repeat(64))).unwrap();
        let operator_address = signer.address();
        EvmChainDriver {
            provider: SeismicProviderBuilder::new()
                .connect_http(Url::parse("http://127.0.0.1:1").unwrap()),
            wallet: SeismicWallet::from(signer),
            operator_address,
            faucet_address: Address::with_last_byte(1),
            operator_key: format!("5124:{operator_address:#x}"),
            chain_id: 5124,
            confirmations: 1,
            receipt_timeout: Duration::from_millis(10),
            erc20_usdc: None,
        }
    }

    fn input() -> FundingInput {
        FundingInput {
            asset: FundingAsset::Susdc,
            deployment_identity: None,
            network_identity: None,
            idempotency_key: "order-123".into(),
            recipient: Address::with_last_byte(0xa1),
            recipient_text: "0x00000000000000000000000000000000000000A1".into(),
            amount: U256::from(12_500_000u64),
            reason: "order_payout".into(),
        }
    }

    fn receipt(status: u8, block_number: Option<u64>) -> SeismicTransactionReceipt {
        let block_number = block_number
            .map(|number| format!("\"0x{number:x}\""))
            .unwrap_or_else(|| "null".into());
        let bloom = "0".repeat(512);
        serde_json::from_str(&format!(
            r#"{{
                "blockHash":"0x{block_hash}",
                "blockNumber":{block_number},
                "contractAddress":null,
                "cumulativeGasUsed":"0x5208",
                "effectiveGasPrice":"0x1",
                "from":"0x0000000000000000000000000000000000000001",
                "gasUsed":"0x5208",
                "logs":[],
                "logsBloom":"0x{bloom}",
                "status":"0x{status:x}",
                "to":"0x0000000000000000000000000000000000000002",
                "transactionHash":"0x{transaction_hash}",
                "transactionIndex":"0x0",
                "type":"0x0"
            }}"#,
            block_hash = "1".repeat(64),
            transaction_hash = "2".repeat(64),
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn signs_legacy_transaction_and_hashes_exact_bytes() {
        let driver = driver();
        let prepared = driver
            .sign_transaction(&input(), 7, 2_000_000_000u128)
            .await
            .unwrap();
        let raw = hex::decode(prepared.serialized_transaction.trim_start_matches("0x")).unwrap();
        assert_eq!(prepared.hash, format!("{:#x}", keccak256(&raw)));
        assert_eq!(prepared.nonce, 7);
        assert!(raw.first().is_some_and(|prefix| *prefix >= 0xc0));

        let calldata = EvmChainDriver::transfer_exact_data(&input());
        assert_eq!(&calldata[..4], &TRANSFER_EXACT_SELECTOR);
        let decoded = SeismicFaucet::transferExactCall::abi_decode(&calldata).unwrap();
        assert_eq!(decoded._recipient, input().recipient);
        assert_eq!(decoded._amount, input().amount);
        assert_eq!(calldata.len(), 68);
    }

    #[test]
    fn gas_request_is_zero_value_susdc_transfer() {
        let driver = driver();
        let mut gas = input();
        gas.asset = FundingAsset::SusdcGas;
        gas.amount = U256::from(10_000u64);
        let request = driver.transaction_request(&gas, 9, 2_000_000_000).unwrap();

        assert_eq!(
            request.inner.transaction_type,
            Some(LEGACY_TRANSACTION_TYPE)
        );
        assert_eq!(request.inner.to, Some(TxKind::Call(driver.faucet_address)));
        assert_eq!(request.inner.value, Some(U256::ZERO));
        let calldata = request.inner.input.input().unwrap();
        let decoded = SeismicFaucet::transferExactCall::abi_decode(calldata).unwrap();
        assert_eq!(decoded._recipient, gas.recipient);
        assert_eq!(decoded._amount, gas.amount);
        assert!(request.seismic_elements.is_none());
    }

    #[test]
    fn erc20_usdc_request_targets_independent_faucet() {
        let mut driver = driver();
        let erc20_faucet = Address::with_last_byte(0x20);
        driver.erc20_usdc = Some(Erc20UsdcConfig {
            token_address: Address::with_last_byte(0x10),
            faucet_address: erc20_faucet,
            private_key: format!("0x{}", "1".repeat(64)),
            funding_address: driver.operator_address,
            reserve_address: Address::with_last_byte(0x30),
            max_amount: U256::from(250_000_000u64),
            global_budget: U256::from(1_000_000_000u64),
            rate_limit: 10,
            rate_window: Duration::from_secs(3_600),
        });
        let mut erc20 = input();
        erc20.asset = FundingAsset::Erc20Usdc;
        erc20.deployment_identity = driver
            .erc20_usdc
            .as_ref()
            .map(|config| config.identity(driver.chain_id));
        let request = driver
            .transaction_request(&erc20, 9, 2_000_000_000)
            .unwrap();

        assert_eq!(request.inner.to, Some(TxKind::Call(erc20_faucet)));
        assert_eq!(request.inner.value, Some(U256::ZERO));
        let calldata = request.inner.input.input().unwrap();
        let decoded = ERC20USDCFaucet::transferExactCall::abi_decode(calldata).unwrap();
        assert_eq!(decoded.recipient, erc20.recipient);
        assert_eq!(decoded.amount, erc20.amount);
    }

    #[test]
    fn rejects_input_from_a_rotated_erc20_deployment() {
        let mut driver = driver();
        driver.erc20_usdc = Some(Erc20UsdcConfig {
            token_address: Address::with_last_byte(0x10),
            faucet_address: Address::with_last_byte(0x20),
            private_key: format!("0x{}", "1".repeat(64)),
            funding_address: driver.operator_address,
            reserve_address: Address::with_last_byte(0x30),
            max_amount: U256::from(250_000_000u64),
            global_budget: U256::from(1_000_000_000u64),
            rate_limit: 10,
            rate_window: Duration::from_secs(3_600),
        });
        let mut erc20 = input();
        erc20.asset = FundingAsset::Erc20Usdc;
        erc20.deployment_identity = Some(crate::model::Erc20DeploymentIdentity {
            chain_id: driver.chain_id,
            token_address: Address::with_last_byte(0x11).to_checksum(None),
            faucet_address: Address::with_last_byte(0x21).to_checksum(None),
            operator_address: driver.operator_address.to_checksum(None),
            reserve_address: Address::with_last_byte(0x30).to_checksum(None),
        });

        let error = driver.validate_input(&erc20).unwrap_err();
        assert_eq!(error.code, "deployment_identity_mismatch");
    }

    #[test]
    fn classifies_terminal_send_errors_without_erasing_provider_text() {
        assert_eq!(
            classify_broadcast_error("insufficient funds for gas"),
            Some(ChainResult::Rejected("insufficient funds for gas".into()))
        );
        assert_eq!(classify_broadcast_error("connection reset"), None);
    }

    #[test]
    fn alloy_receipts_keep_ambiguous_and_terminal_states_distinct() {
        let successful = receipt(1, Some(12));
        let reverted = receipt(0, Some(12));
        let unmined = receipt(1, None);
        let mut incomplete = receipt(1, Some(12));
        incomplete.block_hash = None;
        assert_eq!(receipt_status(None), None);
        assert_eq!(receipt_status(Some(&unmined)), None);
        assert_eq!(receipt_status(Some(&incomplete)), None);
        assert_eq!(receipt_status(Some(&reverted)), Some(ChainResult::Reverted));
        assert_eq!(
            receipt_status(Some(&successful)),
            Some(ChainResult::Success)
        );
    }
}
