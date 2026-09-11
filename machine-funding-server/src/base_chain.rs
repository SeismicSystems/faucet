//! Chain driver for Base Sepolia: a standard EVM network where the reserve
//! key sends native ETH gas drips and plain ERC20 USDC transfers itself, with
//! no faucet contract in between. Transactions are EIP-1559, signed locally,
//! and persisted before broadcast like the Seismic driver's.

use crate::{
    chain::{
        chain_error, classify_broadcast_error, deployment_identity_error, rpc_timeout, ChainDriver,
    },
    config::BaseConfig,
    model::{
        ChainResult, FundingAsset, FundingInput, PreparedTransaction, ReserveDiagnostic,
        ServiceError,
    },
};
use alloy_consensus::{transaction::SignerRecoverable, Transaction, TxEnvelope};
use alloy_eips::eip2718::{Decodable2718, Encodable2718};
use alloy_network::{Ethereum, EthereumWallet, TransactionBuilder};
use alloy_primitives::{keccak256, Bytes, TxKind, B256, U256};
use alloy_provider::{Provider, RootProvider};
use alloy_rpc_types_eth::{TransactionInput, TransactionReceipt, TransactionRequest};
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{sol, SolCall};
use async_trait::async_trait;
use std::{str::FromStr, time::Duration};
use tokio::time::{sleep, timeout};
use url::Url;

const ETH_TRANSFER_GAS_LIMIT: u64 = 21_000;
const MAX_ETH_TRANSFER_GAS_LIMIT: u64 = 100_000;
const GAS_HEADROOM_DIVISOR: u64 = 4;
const ERC20_TRANSFER_GAS_LIMIT: u64 = 120_000;
const FEE_HEADROOM_MULTIPLIER: u128 = 2;
const RECEIPT_POLL_INTERVAL: Duration = Duration::from_millis(500);
const EXPECTED_TOKEN_DECIMALS: u8 = 6;

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function decimals() external view returns (uint8);
        function balanceOf(address account) external view returns (uint256);
        function transfer(address to, uint256 amount) external returns (bool);
    }
}

#[derive(Clone)]
pub struct BaseChainDriver {
    provider: RootProvider<Ethereum>,
    wallet: EthereumWallet,
    config: BaseConfig,
    operator_key: String,
    receipt_timeout: Duration,
}

impl BaseChainDriver {
    /// Connect and run the full preflight: chain id, signer/address match,
    /// token code, six decimals, and both reserves above their floors.
    pub async fn connect(
        config: &BaseConfig,
        receipt_timeout: Duration,
    ) -> Result<Self, ServiceError> {
        let rpc_url = Url::parse(&config.rpc_url).map_err(chain_error)?;
        let provider = RootProvider::<Ethereum>::new_http(rpc_url);
        let chain_id = rpc_timeout(provider.get_chain_id()).await?;
        if chain_id != config.chain_id {
            return Err(chain_error(format!(
                "Base RPC chain id {chain_id} does not match BASE_CHAIN_ID {}",
                config.chain_id
            )));
        }
        let signer = PrivateKeySigner::from_str(&config.private_key).map_err(chain_error)?;
        if signer.address() != config.reserve_address {
            return Err(chain_error(
                "INTERNAL_FUNDING_BASE_PRIVATE_KEY does not match INTERNAL_FUNDING_BASE_ADDRESS",
            ));
        }
        let driver = Self {
            provider,
            wallet: EthereumWallet::from(signer),
            operator_key: format!("{chain_id}:{:#x}", config.reserve_address),
            config: config.clone(),
            receipt_timeout,
        };
        driver.validate_token().await?;
        driver.require_reserves().await?;
        Ok(driver)
    }

    pub fn config(&self) -> &BaseConfig {
        &self.config
    }

    async fn validate_token(&self) -> Result<(), ServiceError> {
        let code =
            rpc_timeout(async { self.provider.get_code_at(self.config.token_address).await })
                .await?;
        if code.is_empty() {
            return Err(chain_error(
                "BASE_ERC20_USDC_TOKEN_ADDRESS has no deployed bytecode",
            ));
        }
        let token = IERC20::new(self.config.token_address, &self.provider);
        let decimals = rpc_timeout(async { token.decimals().call().await }).await?;
        if decimals != EXPECTED_TOKEN_DECIMALS {
            return Err(chain_error(format!(
                "Base ERC20 USDC token has {decimals} decimals; expected {EXPECTED_TOKEN_DECIMALS}"
            )));
        }
        Ok(())
    }

    /// Reads both reserve balances and compares them to the configured floors.
    pub async fn reserves(&self) -> Result<ReserveDiagnostic, ServiceError> {
        let native_balance =
            rpc_timeout(async { self.provider.get_balance(self.config.reserve_address).await })
                .await?;
        let token = IERC20::new(self.config.token_address, &self.provider);
        let erc20_usdc_balance =
            rpc_timeout(async { token.balanceOf(self.config.reserve_address).call().await })
                .await?;
        let diagnostic = ReserveDiagnostic {
            chain_id: self.config.chain_id,
            reserve_address: self.config.reserve_address.to_checksum(None),
            native_balance: native_balance.to_string(),
            native_floor: self.config.eth_reserve_floor.to_string(),
            native_low: native_balance < self.config.eth_reserve_floor,
            erc20_usdc_balance: erc20_usdc_balance.to_string(),
            erc20_usdc_floor: self.config.erc20_usdc_reserve_floor.to_string(),
            erc20_usdc_low: erc20_usdc_balance < self.config.erc20_usdc_reserve_floor,
        };
        if diagnostic.is_low() {
            tracing::warn!(
                native_balance = %diagnostic.native_balance,
                native_floor = %diagnostic.native_floor,
                erc20_usdc_balance = %diagnostic.erc20_usdc_balance,
                erc20_usdc_floor = %diagnostic.erc20_usdc_floor,
                "Base reserve is below its configured floor"
            );
        }
        Ok(diagnostic)
    }

    async fn require_reserves(&self) -> Result<(), ServiceError> {
        let diagnostic = self.reserves().await?;
        if diagnostic.native_low {
            return Err(chain_error(format!(
                "Base reserve ETH balance {} is below floor {}",
                diagnostic.native_balance, diagnostic.native_floor
            )));
        }
        if diagnostic.erc20_usdc_low {
            return Err(chain_error(format!(
                "Base reserve ERC20 USDC balance {} is below floor {}",
                diagnostic.erc20_usdc_balance, diagnostic.erc20_usdc_floor
            )));
        }
        Ok(())
    }

    fn transaction_request(
        &self,
        input: &FundingInput,
        nonce: u64,
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
    ) -> Result<TransactionRequest, ServiceError> {
        let (to, value, data, gas) = match input.asset {
            FundingAsset::BaseEth => (input.recipient, input.amount, Bytes::new(), None),
            FundingAsset::BaseErc20Usdc => (
                self.config.token_address,
                U256::ZERO,
                IERC20::transferCall {
                    to: input.recipient,
                    amount: input.amount,
                }
                .abi_encode()
                .into(),
                Some(ERC20_TRANSFER_GAS_LIMIT),
            ),
            FundingAsset::Susdc | FundingAsset::SusdcGas | FundingAsset::Erc20Usdc => {
                return Err(deployment_identity_error());
            }
        };
        Ok(TransactionRequest {
            from: Some(self.config.reserve_address),
            to: Some(TxKind::Call(to)),
            max_fee_per_gas: Some(max_fee_per_gas),
            max_priority_fee_per_gas: Some(max_priority_fee_per_gas),
            gas,
            value: Some(value),
            input: TransactionInput::from(data),
            nonce: Some(nonce),
            chain_id: Some(self.config.chain_id),
            ..Default::default()
        })
    }

    async fn sign_transaction(
        &self,
        input: &FundingInput,
        nonce: u64,
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
    ) -> Result<PreparedTransaction, ServiceError> {
        let mut request =
            self.transaction_request(input, nonce, max_fee_per_gas, max_priority_fee_per_gas)?;
        if input.asset == FundingAsset::BaseEth {
            let estimate =
                rpc_timeout(async { self.provider.estimate_gas(request.clone()).await }).await?;
            request.gas = Some(eth_gas_limit(estimate)?);
        }
        let envelope = TransactionBuilder::<Ethereum>::build(request, &self.wallet)
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
                        let required_block = receipt_block
                            .saturating_add(self.config.confirmations.saturating_sub(1));
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
}

#[async_trait]
impl ChainDriver for BaseChainDriver {
    fn operator_key(&self) -> &str {
        &self.operator_key
    }

    async fn can_retry_reverted(
        &self,
        input: &FundingInput,
        transaction: &PreparedTransaction,
    ) -> Result<bool, ServiceError> {
        self.validate_input(input)?;
        if !legacy_eth_transfer(input, transaction, &self.config)? {
            return Ok(false);
        }
        let hash = B256::from_str(&transaction.hash).map_err(chain_error)?;
        let receipt = rpc_timeout(self.provider.get_transaction_receipt(hash)).await?;
        let Some(receipt) = receipt else {
            return Ok(false);
        };
        if receipt.transaction_hash != hash
            || receipt_status(Some(&receipt)) != Some(ChainResult::Reverted)
        {
            return Ok(false);
        }
        let latest = rpc_timeout(self.provider.get_block_number()).await?;
        Ok(latest
            >= receipt
                .block_number
                .unwrap()
                .saturating_add(self.config.confirmations.saturating_sub(1)))
    }

    /// Only Base assets bound to this exact deployment are signed; a request
    /// persisted under a rotated token or reserve is rejected before signing.
    fn validate_input(&self, input: &FundingInput) -> Result<(), ServiceError> {
        if input.asset.is_base()
            && input.deployment_identity.is_none()
            && input.network_identity.as_ref() == Some(&self.config.identity())
        {
            return Ok(());
        }
        Err(deployment_identity_error())
    }

    async fn pending_nonce(&self) -> Result<u64, ServiceError> {
        rpc_timeout(async {
            self.provider
                .get_transaction_count(self.config.reserve_address)
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
        let estimate = rpc_timeout(self.provider.estimate_eip1559_fees()).await?;
        let max_fee_per_gas = estimate
            .max_fee_per_gas
            .checked_mul(FEE_HEADROOM_MULTIPLIER)
            .ok_or_else(|| chain_error("RPC fee estimate overflow"))?;
        self.sign_transaction(
            input,
            nonce,
            max_fee_per_gas,
            estimate.max_priority_fee_per_gas,
        )
        .await
    }

    async fn broadcast_and_confirm(
        &self,
        transaction: &PreparedTransaction,
    ) -> Result<ChainResult, ServiceError> {
        let raw = hex::decode(transaction.serialized_transaction.trim_start_matches("0x"))
            .map_err(chain_error)?;
        let expected_hash = B256::from_str(&transaction.hash).map_err(chain_error)?;
        match timeout(
            crate::chain::RPC_REQUEST_TIMEOUT,
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
                tracing::warn!(%message, hash = %transaction.hash, "Base raw transaction broadcast was not acknowledged");
            }
            Err(_) => {
                tracing::warn!(hash = %transaction.hash, "Base raw transaction broadcast timed out");
            }
        }
        self.receipt_result(expected_hash).await
    }

    async fn health(&self) -> Result<(), ServiceError> {
        let chain_id = rpc_timeout(self.provider.get_chain_id()).await?;
        if chain_id != self.config.chain_id {
            return Err(chain_error(format!(
                "Base RPC chain id changed from {} to {chain_id}",
                self.config.chain_id
            )));
        }
        self.validate_token().await?;
        self.require_reserves().await
    }

    async fn reserve_diagnostic(&self) -> Result<Option<ReserveDiagnostic>, ServiceError> {
        self.reserves().await.map(Some)
    }
}

fn eth_gas_limit(estimate: u64) -> Result<u64, ServiceError> {
    let limit = estimate
        .checked_add(estimate.div_ceil(GAS_HEADROOM_DIVISOR))
        .filter(|limit| *limit <= MAX_ETH_TRANSFER_GAS_LIMIT && estimate >= ETH_TRANSFER_GAS_LIMIT)
        .ok_or_else(|| {
            chain_error("Base ETH transfer gas estimate exceeds the supported budget")
        })?;
    Ok(limit)
}

fn legacy_eth_transfer(
    input: &FundingInput,
    transaction: &PreparedTransaction,
    config: &BaseConfig,
) -> Result<bool, ServiceError> {
    if input.asset != FundingAsset::BaseEth {
        return Ok(false);
    }
    let raw = hex::decode(transaction.serialized_transaction.trim_start_matches("0x"))
        .map_err(chain_error)?;
    let envelope = TxEnvelope::decode_2718(&mut raw.as_slice()).map_err(chain_error)?;
    Ok(format!("{:#x}", keccak256(&raw)) == transaction.hash
        && envelope.gas_limit() == ETH_TRANSFER_GAS_LIMIT
        && envelope.chain_id() == Some(config.chain_id)
        && envelope.nonce() == transaction.nonce
        && envelope.to() == Some(input.recipient)
        && envelope.value() == input.amount
        && envelope.input().is_empty()
        && envelope.recover_signer().map_err(chain_error)? == config.reserve_address)
}

fn receipt_status(receipt: Option<&TransactionReceipt>) -> Option<ChainResult> {
    let receipt = receipt?;
    receipt.block_number?;
    receipt.block_hash?;
    if receipt.status() {
        Some(ChainResult::Success)
    } else {
        Some(ChainResult::Reverted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_network::TxSigner;
    use alloy_primitives::Address;
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::{json, Value};

    async fn gas_rpc(Json(request): Json<Value>) -> Json<Value> {
        assert_eq!(request["method"], "eth_estimateGas");
        assert!(request["params"][0].get("gas").is_none());
        Json(json!({"jsonrpc":"2.0", "id":request["id"], "result":"0x52e4"}))
    }

    async fn estimated_driver() -> (BaseChainDriver, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/", post(gas_rpc)))
                .await
                .unwrap();
        });
        let mut driver = driver();
        driver.provider = RootProvider::new_http(Url::parse(&url).unwrap());
        (driver, task)
    }

    async fn recovery_rpc(State(receipt): State<Value>, Json(request): Json<Value>) -> Json<Value> {
        let result = match request["method"].as_str().unwrap() {
            "eth_getTransactionReceipt" => receipt,
            "eth_blockNumber" => json!("0xc"),
            other => panic!("unexpected RPC {other}"),
        };
        Json(json!({"jsonrpc":"2.0", "id":request["id"], "result":result}))
    }

    async fn signed_legacy(
        driver: &BaseChainDriver,
        input: &FundingInput,
        gas: u64,
    ) -> PreparedTransaction {
        let mut request = driver
            .transaction_request(input, 7, 2_000_000_000, 1_000_000)
            .unwrap();
        request.gas = Some(gas);
        let envelope = TransactionBuilder::<Ethereum>::build(request, &driver.wallet)
            .await
            .unwrap();
        let raw = envelope.encoded_2718();
        PreparedTransaction {
            hash: format!("{:#x}", keccak256(&raw)),
            nonce: 7,
            serialized_transaction: format!("0x{}", hex::encode(raw)),
        }
    }

    #[test]
    fn native_gas_margin_is_bounded_and_never_truncated() {
        assert_eq!(eth_gas_limit(21_000).unwrap(), 26_250);
        assert_eq!(eth_gas_limit(21_220).unwrap(), 26_525);
        assert_eq!(eth_gas_limit(80_000).unwrap(), MAX_ETH_TRANSFER_GAS_LIMIT);
        for estimate in [0, 20_999, 80_001, u64::MAX] {
            assert!(eth_gas_limit(estimate).is_err());
        }
    }

    #[tokio::test]
    async fn legacy_retry_requires_matching_signed_transfer_and_confirmed_revert() {
        let mut driver = driver();
        let input = input(FundingAsset::BaseEth, 10_000_000_000_000);
        let old = signed_legacy(&driver, &input, ETH_TRANSFER_GAS_LIMIT).await;
        assert!(legacy_eth_transfer(&input, &old, &driver.config).unwrap());
        let new = signed_legacy(&driver, &input, 26_525).await;
        assert!(!legacy_eth_transfer(&input, &new, &driver.config).unwrap());
        let mut changed = input.clone();
        changed.amount += U256::from(1);
        assert!(!legacy_eth_transfer(&changed, &old, &driver.config).unwrap());
        for (status, block, confirmations, expected) in [
            (0, Some(12), 1, true),
            (1, Some(12), 1, false),
            (0, None, 1, false),
            (0, Some(12), 2, false),
        ] {
            let receipt = json!({
                "blockHash": format!("0x{}", "1".repeat(64)),
                "blockNumber": block.map(|n| format!("0x{n:x}")),
                "transactionHash": old.hash,
                "transactionIndex":"0x0", "type":"0x2", "contractAddress":null,
                "cumulativeGasUsed":"0x5208", "gasUsed":"0x5208", "effectiveGasPrice":"0x1",
                "logs":[], "logsBloom":format!("0x{}", "0".repeat(512)),
                "status":format!("0x{status:x}"), "from":driver.config.reserve_address, "to":input.recipient
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            driver.provider = RootProvider::new_http(
                Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap(),
            );
            driver.config.confirmations = confirmations;
            let task = tokio::spawn(async move {
                axum::serve(
                    listener,
                    Router::new()
                        .route("/", post(recovery_rpc))
                        .with_state(receipt),
                )
                .await
                .unwrap();
            });
            assert_eq!(
                driver.can_retry_reverted(&input, &old).await.unwrap(),
                expected
            );
            task.abort();
        }
    }

    #[tokio::test]
    async fn delegated_wallet_drip_uses_estimated_gas_with_headroom() {
        let (driver, task) = estimated_driver().await;
        let input = input(FundingAsset::BaseEth, 10_000_000_000_000);
        let prepared = driver
            .sign_transaction(&input, 7, 2_000_000_000, 1_000_000)
            .await
            .unwrap();
        let raw = hex::decode(prepared.serialized_transaction.trim_start_matches("0x")).unwrap();
        let envelope = TxEnvelope::decode_2718(&mut raw.as_slice()).unwrap();
        task.abort();
        assert_eq!(envelope.gas_limit(), 26_525);
        assert_eq!(envelope.value(), input.amount);
    }

    fn base_config(reserve: Address) -> BaseConfig {
        BaseConfig {
            rpc_url: "http://127.0.0.1:1".into(),
            chain_id: 84532,
            private_key: format!("0x{}", "3".repeat(64)),
            reserve_address: reserve,
            token_address: Address::with_last_byte(0x10),
            gas_eth_amount: U256::from(1_000_000_000_000_000u64),
            max_erc20_usdc_amount: U256::from(250_000_000u64),
            global_gas_eth_budget: U256::from(100_000_000_000_000_000u64),
            global_erc20_usdc_budget: U256::from(1_000_000_000u64),
            eth_reserve_floor: U256::from(1_000_000_000_000_000u64),
            erc20_usdc_reserve_floor: U256::from(250_000_000u64),
            rate_limit: 10,
            rate_window: Duration::from_secs(3_600),
            confirmations: 1,
        }
    }

    fn driver() -> BaseChainDriver {
        let signer = PrivateKeySigner::from_str(&format!("0x{}", "3".repeat(64))).unwrap();
        let reserve = signer.address();
        BaseChainDriver {
            provider: RootProvider::<Ethereum>::new_http(Url::parse("http://127.0.0.1:1").unwrap()),
            wallet: EthereumWallet::from(signer),
            config: base_config(reserve),
            operator_key: format!("84532:{reserve:#x}"),
            receipt_timeout: Duration::from_millis(10),
        }
    }

    fn input(asset: FundingAsset, amount: u64) -> FundingInput {
        FundingInput {
            asset,
            deployment_identity: None,
            network_identity: Some(driver().config.identity()),
            idempotency_key: "base-order-123".into(),
            recipient: Address::with_last_byte(0xa1),
            recipient_text: "0x00000000000000000000000000000000000000A1".into(),
            amount: U256::from(amount),
            reason: "order_payout".into(),
        }
    }

    #[tokio::test]
    async fn eth_gas_drip_is_a_plain_value_transfer_signed_by_the_reserve() {
        let (driver, task) = estimated_driver().await;
        let gas = input(FundingAsset::BaseEth, 1_000_000_000_000_000);
        let prepared = driver
            .sign_transaction(&gas, 7, 2_000_000_000, 1_000_000)
            .await
            .unwrap();
        let raw = hex::decode(prepared.serialized_transaction.trim_start_matches("0x")).unwrap();
        assert_eq!(prepared.hash, format!("{:#x}", keccak256(&raw)));
        let envelope = TxEnvelope::decode_2718(&mut raw.as_slice()).unwrap();
        assert!(envelope.is_eip1559());
        assert_eq!(envelope.chain_id(), Some(84532));
        assert_eq!(envelope.nonce(), 7);
        assert_eq!(envelope.to(), Some(gas.recipient));
        assert_eq!(envelope.value(), gas.amount);
        assert!(envelope.input().is_empty());
        assert_eq!(envelope.gas_limit(), 26_525);
        task.abort();
        assert_eq!(
            envelope.recover_signer().unwrap(),
            driver.config.reserve_address
        );
    }

    #[test]
    fn erc20_transfer_targets_the_token_with_zero_value() {
        let driver = driver();
        let usdc = input(FundingAsset::BaseErc20Usdc, 12_500_000);
        let request = driver
            .transaction_request(&usdc, 9, 2_000_000_000, 1_000_000)
            .unwrap();
        assert_eq!(request.to, Some(TxKind::Call(driver.config.token_address)));
        assert_eq!(request.value, Some(U256::ZERO));
        assert_eq!(request.gas, Some(ERC20_TRANSFER_GAS_LIMIT));
        assert_eq!(request.chain_id, Some(84532));
        let calldata = request.input.input().unwrap();
        let decoded = IERC20::transferCall::abi_decode(calldata).unwrap();
        assert_eq!(decoded.to, usdc.recipient);
        assert_eq!(decoded.amount, usdc.amount);
    }

    #[test]
    fn only_base_assets_bound_to_this_deployment_are_accepted() {
        let driver = driver();
        assert!(driver
            .validate_input(&input(FundingAsset::BaseEth, 1))
            .is_ok());
        assert!(driver
            .validate_input(&input(FundingAsset::BaseErc20Usdc, 1))
            .is_ok());

        let mut seismic = input(FundingAsset::Susdc, 1);
        seismic.network_identity = None;
        assert_eq!(
            driver.validate_input(&seismic).unwrap_err().code,
            "deployment_identity_mismatch"
        );
        assert!(driver.transaction_request(&seismic, 1, 1, 1).is_err());

        let mut rotated = input(FundingAsset::BaseErc20Usdc, 1);
        rotated.network_identity = Some(crate::model::BaseNetworkIdentity {
            token_address: Address::with_last_byte(0x11).to_checksum(None),
            ..driver.config.identity()
        });
        assert_eq!(
            driver.validate_input(&rotated).unwrap_err().code,
            "deployment_identity_mismatch"
        );

        let mut unscoped = input(FundingAsset::BaseEth, 1);
        unscoped.network_identity = None;
        assert!(driver.validate_input(&unscoped).is_err());
    }

    #[test]
    fn wallet_signs_for_the_configured_reserve() {
        let driver = driver();
        assert_eq!(
            TxSigner::<alloy_primitives::Signature>::address(
                &PrivateKeySigner::from_str(&driver.config.private_key).unwrap()
            ),
            driver.config.reserve_address
        );
    }

    #[test]
    fn receipts_distinguish_pending_success_and_revert() {
        fn receipt(status: u8, block_number: Option<u64>) -> TransactionReceipt {
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
                    "type":"0x2"
                }}"#,
                block_hash = "1".repeat(64),
                transaction_hash = "2".repeat(64),
            ))
            .unwrap()
        }
        assert_eq!(receipt_status(None), None);
        assert_eq!(receipt_status(Some(&receipt(1, None))), None);
        assert_eq!(
            receipt_status(Some(&receipt(0, Some(12)))),
            Some(ChainResult::Reverted)
        );
        assert_eq!(
            receipt_status(Some(&receipt(1, Some(12)))),
            Some(ChainResult::Success)
        );
    }
}
