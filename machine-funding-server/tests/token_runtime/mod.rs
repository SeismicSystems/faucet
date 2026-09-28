use super::*;
use faucet_machine_funding::assets::{FundingStrategy, FundingToken};
use faucet_machine_funding::model::ReservationResult;
use faucet_machine_funding::store::Reservation;
use std::sync::atomic::{AtomicBool, Ordering};

fn setup(url: String, results: Vec<ChainResult>) -> (Config, FakeDriver, FundingInput) {
    let mut config = config(url);
    enable_base(&mut config);
    let token_address = Address::with_last_byte(0x77);
    let BaseActivation::Enabled(base) = &mut config.base else {
        unreachable!()
    };
    base.additional_erc20_tokens.push(FundingToken {
        contract_address: token_address,
        decimals: 6,
        max_transfer: U256::from(20),
        global_budget: U256::from(20),
        reserve_floor: U256::ZERO,
        funding: FundingStrategy::CallerMintWholeTokens {
            max_tokens_per_call: 100,
            max_calls: 4,
        },
    });
    let mut input = base_request(&config, FundingAsset::BaseErc20Usdc, "token-payout", 12);
    input.network_identity = Some(config.base.enabled().unwrap().token_identity(token_address));
    let mut driver = base_driver(&config, results);
    driver.additional_tokens.push(token_address);
    driver.mint_count = 2;
    (config, driver, input)
}

#[tokio::test]
async fn pending_mint_restart_resumes_original_batch_and_shared_queue() {
    let redis = TestRedis::start().await;
    let (config, driver, input) = setup(
        redis.url.clone(),
        vec![
            ChainResult::Success,
            ChainResult::Pending,
            ChainResult::Success,
            ChainResult::Success,
            ChainResult::Success,
        ],
    );
    let service = FundingService::new(redis.store().await, driver.clone(), config.clone());
    assert_eq!(
        service.execute(input.clone()).await.unwrap_err().code,
        "transaction_pending"
    );
    assert_eq!(driver.counts(), (3, 2));
    let restarted = FundingService::new(redis.store().await, driver.clone(), config.clone());
    // A gas request uses the same signer queue and must finish this token batch first.
    let gas = base_request(
        &config,
        FundingAsset::BaseEth,
        "gas-behind-mint",
        BASE_GAS_WEI,
    );
    assert_eq!(
        restarted.execute(gas.clone()).await.unwrap_err().code,
        "request_queued"
    );
    let response = restarted.execute(input.clone()).await.unwrap();
    assert!(response.replayed);
    assert_eq!(response.transaction_hash, format!("0x{:064x}", 3));
    restarted.execute(gas).await.unwrap();
    let state = driver.inner.lock().unwrap();
    assert_eq!(state.prepared.len(), 4);
    assert_eq!(
        state
            .broadcasts
            .iter()
            .map(|tx| tx.nonce)
            .collect::<Vec<_>>(),
        [0, 1, 1, 2, 3]
    );
    assert_eq!(state.broadcasts[1], state.broadcasts[2]);
}

#[tokio::test]
async fn mint_failure_never_sends_payout_or_replays_mints() {
    let redis = TestRedis::start().await;
    let (config, driver, input) = setup(
        redis.url.clone(),
        vec![ChainResult::Success, ChainResult::Reverted],
    );
    let service = FundingService::new(redis.store().await, driver.clone(), config);
    for _ in 0..2 {
        assert_eq!(
            service.execute(input.clone()).await.unwrap_err().code,
            "mint_reverted"
        );
    }
    assert_eq!(driver.counts(), (3, 2));
}

#[tokio::test]
async fn manual_inventory_failure_does_not_block_other_tokens() {
    let redis = TestRedis::start().await;
    let (config, mut driver, input) = setup(redis.url.clone(), vec![ChainResult::Success]);
    driver.preparation_error = Some("manual_funding_required");
    let service = FundingService::new(redis.store().await, driver.clone(), config.clone());
    for _ in 0..2 {
        assert_eq!(
            service.execute(input.clone()).await.unwrap_err().code,
            "manual_funding_required"
        );
    }
    service
        .execute(base_request(
            &config,
            FundingAsset::BaseErc20Usdc,
            "default",
            12,
        ))
        .await
        .unwrap();
    assert_eq!(driver.counts(), (1, 1));
}

#[tokio::test]
async fn budgets_and_idempotency_are_scoped_by_exact_token() {
    let redis = TestRedis::start().await;
    let (config, mut driver, input) = setup(redis.url.clone(), vec![]);
    driver.mint_count = 0;
    let service = FundingService::new(redis.store().await, driver.clone(), config.clone());
    service.execute(input.clone()).await.unwrap();
    assert!(service.execute(input.clone()).await.unwrap().replayed);
    let mut conflict = input.clone();
    conflict.amount = U256::from(11);
    assert_eq!(
        service.execute(conflict).await.unwrap_err().code,
        "idempotency_conflict"
    );
    let mut second = input.clone();
    second.idempotency_key = "over-budget".into();
    assert_eq!(
        service.execute(second).await.unwrap_err().code,
        "global_budget_exceeded"
    );
    let default = base_request(
        &config,
        FundingAsset::BaseErc20Usdc,
        &input.idempotency_key,
        12,
    );
    service.execute(default).await.unwrap();
    assert_eq!(driver.counts(), (2, 2));
}

#[tokio::test]
async fn receipt_success_without_matching_transfer_is_not_funding_success() {
    let redis = TestRedis::start().await;
    let (config, mut driver, input) = setup(redis.url.clone(), vec![]);
    driver.mint_count = 0;
    driver.valid_payout = false;
    let service = FundingService::new(redis.store().await, driver.clone(), config);
    assert_eq!(
        service.execute(input.clone()).await.unwrap_err().code,
        "transaction_rejected"
    );
    assert_eq!(
        service.execute(input).await.unwrap_err().code,
        "transaction_rejected"
    );
    assert_eq!(driver.counts(), (1, 1));
}

#[derive(Clone)]
struct FaultStore {
    inner: RedisStore,
    fail: Arc<AtomicBool>,
    stage: usize,
}
#[async_trait]
impl FundingStore for FaultStore {
    async fn reserve(&self, r: Reservation<'_>) -> Result<ReservationResult, ServiceError> {
        self.inner.reserve(r).await
    }
    async fn get_record(&self, key: &str) -> Result<Option<FundingRecord>, ServiceError> {
        self.inner.get_record(key).await
    }
    async fn set_record(&self, key: &str, record: &FundingRecord) -> Result<(), ServiceError> {
        let hit = match record {
            FundingRecord::Replenishing { next_mint, .. } => *next_mint == self.stage,
            FundingRecord::Prepared { .. } => self.stage == 3,
            _ => false,
        };
        if hit && self.fail.swap(false, Ordering::SeqCst) {
            return Err(ServiceError::new(
                503,
                "injected_store_failure",
                "test failure",
            ));
        }
        self.inner.set_record(key, record).await
    }
    async fn retry_reverted(
        &self,
        key: &str,
        queue: &str,
        failed: &FundingRecord,
    ) -> Result<(), ServiceError> {
        self.inner.retry_reverted(key, queue, failed).await
    }
    async fn queue_head(&self, key: &str) -> Result<Option<String>, ServiceError> {
        self.inner.queue_head(key).await
    }
    async fn pop_queue_head(&self, key: &str, expected: &str) -> Result<(), ServiceError> {
        self.inner.pop_queue_head(key, expected).await
    }
    async fn acquire_lock(&self, key: &str, token: &str, ttl: u64) -> Result<bool, ServiceError> {
        self.inner.acquire_lock(key, token, ttl).await
    }
    async fn release_lock(&self, key: &str, token: &str) -> Result<(), ServiceError> {
        self.inner.release_lock(key, token).await
    }
    async fn renew_lock(&self, key: &str, token: &str, ttl: u64) -> Result<bool, ServiceError> {
        self.inner.renew_lock(key, token, ttl).await
    }
    async fn ping(&self) -> Result<(), ServiceError> {
        self.inner.ping().await
    }
    async fn verify_durability(&self) -> Result<(), ServiceError> {
        self.inner.verify_durability().await
    }
}

#[tokio::test]
async fn persistence_failures_before_broadcast_and_after_confirmation_recover_safely() {
    for stage in 0..=3 {
        let redis = TestRedis::start().await;
        let (config, driver, input) = setup(redis.url.clone(), vec![]);
        let store = FaultStore {
            inner: redis.store().await,
            fail: Arc::new(AtomicBool::new(true)),
            stage,
        };
        let service = FundingService::new(store, driver.clone(), config.clone());
        assert_eq!(
            service.execute(input.clone()).await.unwrap_err().code,
            "injected_store_failure"
        );
        let original = {
            let state = driver.inner.lock().unwrap();
            assert_eq!(state.broadcasts.len(), stage.min(2));
            state.prepared.clone()
        };
        let restarted = FundingService::new(redis.store().await, driver.clone(), config);
        restarted.execute(input).await.unwrap();
        let state = driver.inner.lock().unwrap();
        if stage != 0 {
            assert_eq!(state.prepared, original);
            assert_eq!(state.broadcasts.last().unwrap(), original.last().unwrap());
        }
        if stage == 1 || stage == 2 {
            assert_eq!(state.broadcasts[stage - 1], state.broadcasts[stage]);
        }
    }
}

#[tokio::test]
async fn token_routes_validate_selection_limits_and_preserve_default_replays() {
    let redis = TestRedis::start().await;
    let (config, mut driver, input) = setup(redis.url.clone(), vec![]);
    driver.mint_count = 0;
    let legacy = FundingService::new(redis.store().await, FakeDriver::new([]), config.clone());
    let service = FundingService::new(redis.store().await, driver.clone(), config.clone());
    let app = router_with_networks(
        legacy,
        Erc20FundingService::Disabled,
        BaseFundingService::Enabled(Box::new(service)),
    );
    let token = input
        .network_identity
        .as_ref()
        .unwrap()
        .token_address
        .clone();
    let auth = "Bearer a-secure-machine-token-with-32-characters";
    for (selected, amount, authorized, expected, code) in [
        (Some(token.clone()), "12", false, 401, Some("unauthorized")),
        (
            Some(Address::with_last_byte(0x99).to_string()),
            "12",
            true,
            400,
            Some("unsupported_token"),
        ),
        (
            Some("invalid".into()),
            "12",
            true,
            400,
            Some("invalid_request"),
        ),
        (
            Some(token.clone()),
            "21",
            true,
            400,
            Some("amount_exceeds_limit"),
        ),
        (Some(token.clone()), "12", true, 200, None),
        (None, "12", true, 200, None),
        (Some(BASE_TOKEN.into()), "12", true, 200, None),
    ] {
        let body = serde_json::json!({"idempotency_key":"same-key", "recipient":RECIPIENT,"amount":amount,"reason":"test","token_address":selected});
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/internal/base/erc20/transfers")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(
                        header::AUTHORIZATION,
                        if authorized { auth } else { "Bearer wrong" },
                    )
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), expected);
        if let Some(code) = code {
            assert_eq!(json_body(response).await["error"]["code"], code);
        } else if selected.as_deref() == Some(BASE_TOKEN) {
            assert_eq!(json_body(response).await["replayed"], true);
        }
    }
    assert_eq!(driver.counts(), (2, 2));
    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/internal/base/settlement?token_address={token}"
            ))
            .header(header::AUTHORIZATION, auth)
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["token_address"],
        token.to_ascii_lowercase()
    );
    let unknown = app
        .oneshot(
            Request::get(format!(
                "/api/internal/base/settlement?token_address={}",
                Address::ZERO
            ))
            .header(header::AUTHORIZATION, auth)
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn removed_token_cannot_resume_under_the_default_identity() {
    let redis = TestRedis::start().await;
    let (config, driver, input) = setup(redis.url.clone(), vec![ChainResult::Pending]);
    let service = FundingService::new(redis.store().await, driver.clone(), config.clone());
    assert_eq!(
        service.execute(input).await.unwrap_err().code,
        "transaction_pending"
    );
    let mut rotated_driver = driver.clone();
    rotated_driver.additional_tokens.clear();
    let restarted = FundingService::new(redis.store().await, rotated_driver, config.clone());
    let default = base_request(
        &config,
        FundingAsset::BaseErc20Usdc,
        "default-after-removal",
        1,
    );
    assert_eq!(
        restarted.execute(default.clone()).await.unwrap_err().code,
        "request_queued"
    );
    restarted.execute(default).await.unwrap();
    let state = driver.inner.lock().unwrap();
    assert_eq!(state.broadcasts.len(), 2);
    assert_eq!(state.prepared.len(), 4);
}
