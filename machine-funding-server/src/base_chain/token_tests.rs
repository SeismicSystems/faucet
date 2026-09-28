use super::*;
use crate::assets::{FundingStrategy, FundingToken};
use alloy_primitives::Address;
use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};

#[derive(Clone)]
struct RpcState {
    balance: u64,
    decimals: u8,
    code: &'static str,
    receipt: Value,
}

async fn rpc(State(state): State<RpcState>, Json(request): Json<Value>) -> Json<Value> {
    let result = match request["method"].as_str().unwrap() {
        "eth_getCode" => json!(state.code),
        "eth_call" => {
            let calldata = request["params"][0]["input"]
                .as_str()
                .or_else(|| request["params"][0]["data"].as_str())
                .unwrap();
            let value = if calldata == "0x313ce567" {
                u64::from(state.decimals)
            } else {
                assert!(calldata.starts_with("0x70a08231"));
                state.balance
            };
            json!(format!("0x{value:064x}"))
        }
        "eth_feeHistory" => {
            json!({"oldestBlock":"0x1","baseFeePerGas":["0x1","0x1"],"gasUsedRatio":[0.5],"reward":[["0x1"]]})
        }
        "eth_estimateGas" => json!("0x10000"),
        "eth_getTransactionReceipt" => state.receipt,
        "eth_blockNumber" => json!("0xc"),
        other => panic!("unexpected RPC call {other}"),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}

async fn driver(state: RpcState) -> (BaseChainDriver, FundingInput, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = RootProvider::<Ethereum>::new_http(
        Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap(),
    );
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/", post(rpc)).with_state(state),
        )
        .await
        .unwrap();
    });
    let signer = PrivateKeySigner::from_str(&format!("0x{}", "3".repeat(64))).unwrap();
    let reserve = signer.address();
    let config = BaseConfig {
        rpc_url: "http://127.0.0.1:1".into(),
        chain_id: 84532,
        private_key: format!("0x{}", "3".repeat(64)),
        reserve_address: reserve,
        token_address: Address::with_last_byte(1),
        additional_erc20_tokens: vec![FundingToken {
            contract_address: Address::with_last_byte(2),
            decimals: 6,
            max_transfer: U256::from(400_000_000),
            global_budget: U256::from(1_000_000_000),
            reserve_floor: U256::ZERO,
            funding: FundingStrategy::CallerMintWholeTokens {
                max_tokens_per_call: 100,
                max_calls: 4,
            },
        }],
        gas_eth_amount: U256::from(1),
        max_erc20_usdc_amount: U256::from(250_000_000),
        global_gas_eth_budget: U256::from(100),
        global_erc20_usdc_budget: U256::from(1_000_000_000),
        eth_reserve_floor: U256::from(1),
        erc20_usdc_reserve_floor: U256::ZERO,
        rate_limit: 10,
        rate_window: Duration::from_secs(60),
        confirmations: 1,
    };
    let input = FundingInput {
        asset: FundingAsset::BaseErc20Usdc,
        deployment_identity: None,
        network_identity: Some(config.token_identity(Address::with_last_byte(2))),
        idempotency_key: "batch".into(),
        recipient: Address::with_last_byte(3),
        recipient_text: Address::with_last_byte(3).to_checksum(None),
        amount: U256::from(250_000_001),
        reason: "test".into(),
    };
    (
        BaseChainDriver {
            provider,
            wallet: EthereumWallet::from(signer),
            config,
            operator_key: format!("84532:{reserve:#x}"),
            receipt_timeout: Duration::from_millis(10),
        },
        input,
        task,
    )
}

fn state(balance: u64) -> RpcState {
    RpcState {
        balance,
        decimals: 6,
        code: "0x6000",
        receipt: Value::Null,
    }
}
fn decode(tx: &PreparedTransaction) -> TxEnvelope {
    TxEnvelope::decode_2718(
        &mut hex::decode(tx.serialized_transaction.trim_start_matches("0x"))
            .unwrap()
            .as_slice(),
    )
    .unwrap()
}

#[tokio::test]
async fn batch_signs_bounded_whole_token_mints_then_exact_payout() {
    let (driver, input, task) = driver(state(0)).await;
    let (mints, payout) = driver.prepare_funding(&input, 7).await.unwrap();
    assert_eq!(mints.len(), 3);
    for (index, (mint, amount)) in mints.iter().zip([100, 100, 51]).enumerate() {
        let envelope = decode(mint);
        assert_eq!(envelope.nonce(), 7 + index as u64);
        assert_eq!(envelope.to(), Some(Address::with_last_byte(2)));
        assert_eq!(
            envelope.recover_signer().unwrap(),
            driver.config.reserve_address
        );
        assert_eq!(envelope.chain_id(), Some(84532));
        assert_eq!(envelope.value(), U256::ZERO);
        assert_eq!(envelope.gas_limit(), 81_920);
        assert_eq!(
            crate::assets::mintCall::abi_decode(envelope.input())
                .unwrap()
                .amount,
            U256::from(amount)
        );
    }
    let payout = decode(&payout);
    assert_eq!(payout.nonce(), 10);
    assert_eq!(payout.to(), Some(Address::with_last_byte(2)));
    let transfer = IERC20::transferCall::abi_decode(payout.input()).unwrap();
    assert_eq!(transfer.to, input.recipient);
    assert_eq!(transfer.amount, input.amount);
    task.abort();
}

#[tokio::test]
async fn manual_inventory_never_mints_and_sufficient_inventory_skips_minting() {
    for balance in [0, 300_000_000] {
        let (mut driver, input, task) = driver(state(balance)).await;
        driver.config.additional_erc20_tokens[0].funding = FundingStrategy::ManualInventory;
        let result = driver.prepare_funding(&input, 7).await;
        if balance == 0 {
            assert_eq!(result.unwrap_err().code, "manual_funding_required");
        } else {
            let (mints, payout) = result.unwrap();
            assert!(mints.is_empty());
            assert_eq!(payout.nonce, 7);
        }
        task.abort();
    }
}

#[tokio::test]
async fn invalid_contract_and_decimals_are_rejected_before_signing() {
    for state in [
        RpcState {
            decimals: 18,
            ..state(0)
        },
        RpcState {
            code: "0x",
            ..state(0)
        },
    ] {
        let (driver, input, task) = driver(state).await;
        assert!(driver.prepare_funding(&input, 0).await.is_err());
        task.abort();
    }
}

#[tokio::test]
async fn payout_requires_an_exact_transfer_event_and_mined_replay_never_broadcasts() {
    for case in 0..4 {
        let reserve = PrivateKeySigner::from_str(&format!("0x{}", "3".repeat(64)))
            .unwrap()
            .address();
        let address = Address::with_last_byte(if case == 1 { 4 } else { 2 });
        let amount = if case == 2 {
            250_000_000u64
        } else {
            250_000_001u64
        };
        let recipient = Address::with_last_byte(if case == 3 { 4 } else { 3 });
        let tx_hash = format!("0x{}", "1".repeat(64));
        let log = json!({"address":address,"topics":[keccak256("Transfer(address,address,uint256)"),reserve.into_word(),recipient.into_word()],"data":format!("0x{amount:064x}"),"blockNumber":"0xc","transactionHash":tx_hash,"transactionIndex":"0x0","blockHash":format!("0x{}","2".repeat(64)),"logIndex":"0x0","removed":false});
        let receipt = json!({"blockHash":format!("0x{}","2".repeat(64)),"blockNumber":"0xc","transactionHash":tx_hash,"transactionIndex":"0x0","type":"0x2","contractAddress":null,"cumulativeGasUsed":"0x10000","gasUsed":"0x10000","effectiveGasPrice":"0x1","logs":[log],"logsBloom":format!("0x{}","0".repeat(512)),"status":"0x1","from":reserve,"to":Address::with_last_byte(2)});
        let (driver, input, task) = driver(RpcState {
            receipt,
            ..state(0)
        })
        .await;
        let tx = PreparedTransaction {
            hash: tx_hash,
            nonce: 0,
            serialized_transaction: "0x01".into(),
        };
        assert_eq!(driver.verify_payout(&input, &tx).await.unwrap(), case == 0);
        // The RPC fixture rejects any eth_sendRawTransaction call.
        assert_eq!(
            driver.broadcast_and_confirm(&tx).await.unwrap(),
            ChainResult::Success
        );
        task.abort();
    }
}

#[tokio::test]
async fn signed_request_identity_survives_a_lowered_admission_limit() {
    let (mut driver, input, task) = driver(state(0)).await;
    driver.config.additional_erc20_tokens[0].max_transfer = U256::from(1);
    assert!(driver.validate_input(&input).is_ok());
    task.abort();
}
