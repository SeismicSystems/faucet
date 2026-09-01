use alloy_primitives::{Address, U256};
use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use faucet_machine_funding::{
    chain::ChainDriver,
    config::{Erc20UsdcActivation, Erc20UsdcConfig},
    model::{
        ChainResult, Erc20DeploymentIdentity, FundingAsset, FundingInput, FundingRecord,
        PersistedInput, PreparedTransaction, ServiceError,
    },
    router, router_with_erc20,
    store::FundingStore,
    Config, Erc20FundingService, FundingService, RedisStore,
};
use http_body_util::BodyExt;
use redis::AsyncCommands;
use std::{
    collections::VecDeque,
    net::{SocketAddr, TcpListener},
    process::{Child, Command, Stdio},
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::TempDir;
use tokio::time::sleep;
use tower::ServiceExt;

const RECIPIENT: &str = "0x00000000000000000000000000000000000000A1";

struct TestRedis {
    child: Child,
    _directory: TempDir,
    url: String,
}

impl TestRedis {
    async fn start() -> Self {
        Self::start_with_durability(true).await
    }

    async fn start_with_durability(durable: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let directory = tempfile::tempdir().unwrap();
        let mut child = Command::new("redis-server")
            .args([
                "--bind",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--save",
                "",
                "--appendonly",
                if durable { "yes" } else { "no" },
                "--appendfsync",
                if durable { "always" } else { "everysec" },
                "--dir",
                directory.path().to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("redis-server must be installed for integration tests");
        let url = format!("redis://127.0.0.1:{port}/");
        for _ in 0..100 {
            if RedisStore::connect(&url).await.is_ok() {
                return Self {
                    child,
                    _directory: directory,
                    url,
                };
            }
            sleep(Duration::from_millis(20)).await;
        }
        let _ = child.kill();
        let _ = child.wait();
        panic!("Redis test server did not start");
    }

    async fn store(&self) -> RedisStore {
        RedisStore::connect(&self.url).await.unwrap()
    }

    async fn raw_connection(&self) -> redis::aio::MultiplexedConnection {
        redis::Client::open(self.url.as_str())
            .unwrap()
            .get_multiplexed_async_connection()
            .await
            .unwrap()
    }
}

impl Drop for TestRedis {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Clone)]
struct FakeDriver {
    inner: Arc<Mutex<FakeState>>,
    operator_key: String,
    deployment_identity: Option<Erc20DeploymentIdentity>,
}

struct FakeState {
    results: VecDeque<ChainResult>,
    prepared: Vec<PreparedTransaction>,
    broadcasts: Vec<PreparedTransaction>,
    prepare_delay: Duration,
}

impl FakeDriver {
    fn new(results: impl IntoIterator<Item = ChainResult>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakeState {
                results: results.into_iter().collect(),
                prepared: Vec::new(),
                broadcasts: Vec::new(),
                prepare_delay: Duration::ZERO,
            })),
            operator_key: "5124:0xmachine".into(),
            deployment_identity: None,
        }
    }

    fn with_operator_key(mut self, operator_key: &str) -> Self {
        self.operator_key = operator_key.into();
        self
    }

    fn with_deployment_identity(mut self, identity: Erc20DeploymentIdentity) -> Self {
        self.deployment_identity = Some(identity);
        self
    }

    fn with_prepare_delay(self, delay: Duration) -> Self {
        self.inner.lock().unwrap().prepare_delay = delay;
        self
    }

    fn counts(&self) -> (usize, usize) {
        let state = self.inner.lock().unwrap();
        (state.prepared.len(), state.broadcasts.len())
    }
}

#[async_trait]
impl ChainDriver for FakeDriver {
    fn operator_key(&self) -> &str {
        &self.operator_key
    }

    fn validate_input(&self, input: &FundingInput) -> Result<(), ServiceError> {
        match (&self.deployment_identity, input.asset) {
            (Some(identity), FundingAsset::Erc20Usdc)
                if input.deployment_identity.as_ref() == Some(identity) =>
            {
                Ok(())
            }
            (None, FundingAsset::Susdc | FundingAsset::SusdcGas)
                if input.deployment_identity.is_none() =>
            {
                Ok(())
            }
            _ => Err(ServiceError::new(
                409,
                "deployment_identity_mismatch",
                "Funding request belongs to a different contract deployment",
            )),
        }
    }

    async fn pending_nonce(&self) -> Result<u64, ServiceError> {
        Ok(self.inner.lock().unwrap().prepared.len() as u64)
    }

    async fn prepare(
        &self,
        _input: &FundingInput,
        nonce: u64,
    ) -> Result<PreparedTransaction, ServiceError> {
        let delay = self.inner.lock().unwrap().prepare_delay;
        sleep(delay).await;
        let transaction = PreparedTransaction {
            hash: format!("0x{:064x}", nonce + 1),
            nonce,
            serialized_transaction: format!("0x{:02x}", nonce + 1),
        };
        self.inner
            .lock()
            .unwrap()
            .prepared
            .push(transaction.clone());
        Ok(transaction)
    }

    async fn broadcast_and_confirm(
        &self,
        transaction: &PreparedTransaction,
    ) -> Result<ChainResult, ServiceError> {
        let mut state = self.inner.lock().unwrap();
        state.broadcasts.push(transaction.clone());
        Ok(state.results.pop_front().unwrap_or(ChainResult::Success))
    }

    async fn health(&self) -> Result<(), ServiceError> {
        Ok(())
    }
}

fn config(redis_url: String) -> Config {
    Config {
        bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        redis_url,
        rpc_url: "http://127.0.0.1:8545".into(),
        chain_id: 5124,
        token: "a-secure-machine-token-with-32-characters".into(),
        private_key: format!("0x{}", "1".repeat(64)),
        funding_address: Address::from_str("0x19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
        faucet_address: Address::with_last_byte(1),
        max_susdc_amount: U256::from(250_000_000u64),
        gas_susdc_amount: U256::from(100_000u64),
        global_susdc_budget: U256::from(1_000_000_000u64),
        global_gas_susdc_budget: U256::from(10_000_000u64),
        erc20_usdc: Erc20UsdcActivation::Disabled,
        rate_limit: 10,
        rate_window: Duration::from_secs(60),
        confirmations: 1,
        receipt_timeout: Duration::from_millis(100),
        lock_wait: Duration::from_secs(1),
        request_timeout: Duration::from_secs(45),
    }
}

fn request(key: &str, amount: u64) -> FundingInput {
    FundingInput {
        asset: FundingAsset::Susdc,
        deployment_identity: None,
        idempotency_key: key.into(),
        recipient: Address::from_str(RECIPIENT).unwrap(),
        recipient_text: RECIPIENT.into(),
        amount: U256::from(amount),
        reason: "order_payout".into(),
    }
}

fn enable_erc20_usdc(config: &mut Config) {
    config.erc20_usdc = Erc20UsdcActivation::Enabled(Erc20UsdcConfig {
        token_address: Address::with_last_byte(0x10),
        faucet_address: Address::with_last_byte(0x20),
        private_key: format!("0x{}", "2".repeat(64)),
        funding_address: Address::with_last_byte(0x40),
        reserve_address: Address::with_last_byte(0x30),
        max_amount: U256::from(250_000_000u64),
        global_budget: U256::from(1_000_000_000u64),
        rate_limit: 10,
        rate_window: Duration::from_secs(60),
    });
}

fn erc20_identity(config: &Config) -> Erc20DeploymentIdentity {
    config
        .erc20_usdc
        .enabled()
        .unwrap()
        .identity(config.chain_id)
}

fn erc20_request(config: &Config, key: &str, amount: u64) -> FundingInput {
    let mut input = request(key, amount);
    input.asset = FundingAsset::Erc20Usdc;
    input.deployment_identity = Some(erc20_identity(config));
    input
}

#[test]
fn legacy_records_without_deployment_identity_remain_readable() {
    let record: FundingRecord = serde_json::from_str(
        r#"{"state":"queued","fingerprint":"legacy","input":{"asset":"susdc","idempotency_key":"legacy-order","recipient":"0x00000000000000000000000000000000000000A1","amount":"12","reason":"order_payout"}}"#,
    )
    .unwrap();
    assert!(matches!(
        record,
        FundingRecord::Queued { input, .. } if input.deployment_identity.is_none()
    ));
}

#[tokio::test]
async fn rejects_redis_without_fsync_always() {
    let redis = TestRedis::start_with_durability(false).await;
    let store = redis.store().await;
    let error = store.verify_durability().await.unwrap_err();
    assert_eq!(error.code, "unsafe_state_configuration");
}

#[tokio::test]
async fn renews_only_the_owned_operator_lock() {
    let redis = TestRedis::start().await;
    let store = redis.store().await;
    assert!(store.acquire_lock("lease", "owner", 50).await.unwrap());
    assert!(!store.renew_lock("lease", "stranger", 500).await.unwrap());
    assert!(store.renew_lock("lease", "owner", 500).await.unwrap());
    sleep(Duration::from_millis(100)).await;
    assert!(!store.acquire_lock("lease", "second", 500).await.unwrap());
    store.release_lock("lease", "owner").await.unwrap();
}

#[tokio::test]
async fn concurrent_retries_sign_and_broadcast_once() {
    let redis = TestRedis::start().await;
    redis.store().await.verify_durability().await.unwrap();
    let driver =
        FakeDriver::new([ChainResult::Success]).with_prepare_delay(Duration::from_millis(50));
    let service = FundingService::new(
        redis.store().await,
        driver.clone(),
        config(redis.url.clone()),
    );
    let (first, second) = tokio::join!(
        service.execute(request("order-123", 12_500_000)),
        service.execute(request("order-123", 12_500_000)),
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.transaction_hash, second.transaction_hash);
    assert_ne!(first.replayed, second.replayed);
    assert_eq!(driver.counts(), (1, 1));
}

#[tokio::test]
async fn hard_request_deadline_releases_operator_lock() {
    let redis = TestRedis::start().await;
    let store = redis.store().await;
    let driver =
        FakeDriver::new([ChainResult::Success]).with_prepare_delay(Duration::from_millis(100));
    let mut deadline = config(redis.url.clone());
    deadline.request_timeout = Duration::from_millis(20);
    let service = FundingService::new(store.clone(), driver, deadline);

    let error = service
        .execute(request("order-123", 12_500_000))
        .await
        .unwrap_err();
    assert_eq!(error.code, "request_timeout");
    sleep(Duration::from_millis(20)).await;
    assert!(store
        .acquire_lock("machine-funding:lock:5124:0xmachine", "probe", 100)
        .await
        .unwrap());
}

#[tokio::test]
async fn pending_transaction_replays_identical_signed_bytes() {
    let redis = TestRedis::start().await;
    let driver = FakeDriver::new([ChainResult::Pending, ChainResult::Success]);
    let service = FundingService::new(
        redis.store().await,
        driver.clone(),
        config(redis.url.clone()),
    );
    let first = service
        .execute(request("order-123", 12_500_000))
        .await
        .unwrap_err();
    assert_eq!(first.code, "transaction_pending");

    let mut connection = redis.raw_connection().await;
    let persisted: String = connection
        .get("machine-funding:idempotency:order-123")
        .await
        .unwrap();
    assert!(persisted.contains("\"serialized_transaction\":\"0x01\""));
    let ttl: i64 = connection
        .ttl("machine-funding:idempotency:order-123")
        .await
        .unwrap();
    assert_eq!(ttl, -1);

    let result = service
        .execute(request("order-123", 12_500_000))
        .await
        .unwrap();
    assert_eq!(result.transaction_hash, format!("0x{:064x}", 1));
    assert_eq!(driver.counts(), (1, 2));
}

#[tokio::test]
async fn reverted_receipt_is_terminal_and_never_rebroadcast() {
    let redis = TestRedis::start().await;
    let driver = FakeDriver::new([ChainResult::Reverted, ChainResult::Success]);
    let service = FundingService::new(
        redis.store().await,
        driver.clone(),
        config(redis.url.clone()),
    );
    let first = service
        .execute(request("order-123", 12_500_000))
        .await
        .unwrap_err();
    let replay = service
        .execute(request("order-123", 12_500_000))
        .await
        .unwrap_err();
    assert_eq!(first.code, "transaction_reverted");
    assert_eq!(replay.code, "transaction_reverted");
    assert_eq!(driver.counts(), (1, 1));
}

#[tokio::test]
async fn terminal_broadcast_rejection_returns_422_and_never_rebroadcasts() {
    let redis = TestRedis::start().await;
    let driver = FakeDriver::new([
        ChainResult::Rejected("insufficient funds for gas".into()),
        ChainResult::Success,
    ]);
    let service = FundingService::new(
        redis.store().await,
        driver.clone(),
        config(redis.url.clone()),
    );
    let first = service
        .execute(request("order-123", 12_500_000))
        .await
        .unwrap_err();
    let replay = service
        .execute(request("order-123", 12_500_000))
        .await
        .unwrap_err();
    assert_eq!(first.status, 422);
    assert_eq!(first.code, "transaction_rejected");
    assert_eq!(first.message, "insufficient funds for gas");
    assert_eq!(replay.code, "transaction_rejected");
    assert_eq!(driver.counts(), (1, 1));
}

#[tokio::test]
async fn enforces_global_budget_atomically() {
    let redis = TestRedis::start().await;
    let driver = FakeDriver::new([ChainResult::Success]);
    let mut limits = config(redis.url.clone());
    limits.global_susdc_budget = U256::from(20u64);
    let service = FundingService::new(redis.store().await, driver.clone(), limits);
    service.execute(request("order-123", 15)).await.unwrap();
    let error = service.execute(request("order-124", 10)).await.unwrap_err();
    assert_eq!(error.code, "global_budget_exceeded");
    assert_eq!(driver.counts(), (1, 1));
}

#[tokio::test]
async fn erc20_usdc_idempotency_is_scoped_to_deployment_identity() {
    let redis = TestRedis::start().await;
    let legacy_driver = FakeDriver::new([ChainResult::Success]);
    let legacy_service = FundingService::new(
        redis.store().await,
        legacy_driver.clone(),
        config(redis.url.clone()),
    );
    let mut erc20_config = config(redis.url.clone());
    enable_erc20_usdc(&mut erc20_config);
    let identity = erc20_identity(&erc20_config);
    let erc20_driver = FakeDriver::new([ChainResult::Success])
        .with_operator_key("5124:0xerc20machine")
        .with_deployment_identity(identity.clone());
    let erc20_service = FundingService::new(
        redis.store().await,
        erc20_driver.clone(),
        erc20_config.clone(),
    );

    legacy_service
        .execute(request("shared-order-key", 12_500_000))
        .await
        .unwrap();
    erc20_service
        .execute(erc20_request(&erc20_config, "shared-order-key", 12_500_000))
        .await
        .unwrap();

    let mut connection = redis.raw_connection().await;
    let susdc_record: Option<String> = connection
        .get("machine-funding:idempotency:shared-order-key")
        .await
        .unwrap();
    let erc20_record: Option<String> = connection
        .get(format!(
            "machine-funding:{}:idempotency:shared-order-key",
            identity.scope()
        ))
        .await
        .unwrap();
    assert!(susdc_record.is_some());
    assert!(erc20_record.is_some());
    assert_eq!(legacy_driver.counts(), (1, 1));
    assert_eq!(erc20_driver.counts(), (1, 1));
}

#[tokio::test]
async fn erc20_usdc_limits_are_independent_from_legacy_limits() {
    let redis = TestRedis::start().await;
    let legacy_driver = FakeDriver::new([ChainResult::Success]);
    let legacy_service = FundingService::new(
        redis.store().await,
        legacy_driver.clone(),
        config(redis.url.clone()),
    );
    let mut erc20_config = config(redis.url.clone());
    enable_erc20_usdc(&mut erc20_config);
    let erc20_limits = match &mut erc20_config.erc20_usdc {
        Erc20UsdcActivation::Enabled(config) => config,
        _ => unreachable!(),
    };
    erc20_limits.global_budget = U256::from(20u64);
    erc20_limits.rate_limit = 1;
    let identity = erc20_identity(&erc20_config);
    let erc20_driver = FakeDriver::new([ChainResult::Success])
        .with_operator_key("5124:0xerc20machine")
        .with_deployment_identity(identity);
    let erc20_service = FundingService::new(
        redis.store().await,
        erc20_driver.clone(),
        erc20_config.clone(),
    );

    erc20_service
        .execute(erc20_request(&erc20_config, "erc20-1", 15))
        .await
        .unwrap();
    let rate_error = erc20_service
        .execute(erc20_request(&erc20_config, "erc20-2", 1))
        .await
        .unwrap_err();
    assert_eq!(rate_error.code, "rate_limited");
    let mut other_recipient = erc20_request(&erc20_config, "erc20-3", 10);
    other_recipient.recipient = Address::with_last_byte(0xa2);
    other_recipient.recipient_text = other_recipient.recipient.to_checksum(None);
    let budget_error = erc20_service.execute(other_recipient).await.unwrap_err();
    assert_eq!(budget_error.code, "global_budget_exceeded");

    legacy_service
        .execute(request("legacy-1", 10))
        .await
        .unwrap();
    assert_eq!(legacy_driver.counts(), (1, 1));
    assert_eq!(erc20_driver.counts(), (1, 1));
}

#[tokio::test]
async fn pending_erc20_transaction_does_not_block_legacy_queue() {
    let redis = TestRedis::start().await;
    let mut erc20_config = config(redis.url.clone());
    enable_erc20_usdc(&mut erc20_config);
    let erc20_driver = FakeDriver::new([ChainResult::Pending])
        .with_operator_key("5124:0xerc20machine")
        .with_deployment_identity(erc20_identity(&erc20_config));
    let erc20_service = FundingService::new(
        redis.store().await,
        erc20_driver.clone(),
        erc20_config.clone(),
    );
    let legacy_driver = FakeDriver::new([ChainResult::Success]);
    let legacy_service = FundingService::new(
        redis.store().await,
        legacy_driver.clone(),
        config(redis.url.clone()),
    );

    let pending = erc20_service
        .execute(erc20_request(&erc20_config, "erc20-pending", 12))
        .await
        .unwrap_err();
    assert_eq!(pending.code, "transaction_pending");
    legacy_service
        .execute(request("legacy-after-erc20", 12))
        .await
        .unwrap();
    assert_eq!(erc20_driver.counts(), (1, 1));
    assert_eq!(legacy_driver.counts(), (1, 1));
}

#[tokio::test]
async fn deployment_rotation_rejects_stale_queue_before_signing() {
    let redis = TestRedis::start().await;
    let mut erc20_config = config(redis.url.clone());
    enable_erc20_usdc(&mut erc20_config);
    let current_identity = erc20_identity(&erc20_config);
    let mut stale_input = erc20_request(&erc20_config, "stale-deployment", 12);
    let stale_identity = Erc20DeploymentIdentity {
        token_address: Address::with_last_byte(0x11).to_checksum(None),
        faucet_address: Address::with_last_byte(0x21).to_checksum(None),
        ..current_identity.clone()
    };
    stale_input.deployment_identity = Some(stale_identity.clone());
    let stale_record = FundingRecord::Queued {
        fingerprint: "stale-fingerprint".into(),
        input: PersistedInput::from(&stale_input),
    };
    let stale_record_key = format!(
        "machine-funding:{}:idempotency:{}",
        stale_identity.scope(),
        stale_input.idempotency_key
    );
    let queue_key = "machine-funding:queue:5124:0xerc20machine";
    let mut connection = redis.raw_connection().await;
    let _: () = connection
        .set(
            &stale_record_key,
            serde_json::to_string(&stale_record).unwrap(),
        )
        .await
        .unwrap();
    let _: usize = connection
        .rpush(queue_key, &stale_record_key)
        .await
        .unwrap();

    let driver = FakeDriver::new([ChainResult::Success])
        .with_operator_key("5124:0xerc20machine")
        .with_deployment_identity(current_identity);
    let service = FundingService::new(redis.store().await, driver.clone(), erc20_config.clone());
    let current = erc20_request(&erc20_config, "current-deployment", 12);
    let queued = service.execute(current.clone()).await.unwrap_err();
    assert_eq!(queued.code, "request_queued");
    assert_eq!(driver.counts(), (0, 0));
    service.execute(current).await.unwrap();
    assert_eq!(driver.counts(), (1, 1));

    let persisted: String = connection.get(stale_record_key).await.unwrap();
    let rejected: FundingRecord = serde_json::from_str(&persisted).unwrap();
    assert!(matches!(
        rejected,
        FundingRecord::Rejected { ref code, .. } if code == "deployment_identity_mismatch"
    ));
}

#[tokio::test]
async fn http_routes_require_auth_and_preserve_success_wire() {
    let redis = TestRedis::start().await;
    let driver = FakeDriver::new([ChainResult::Success]);
    let service = FundingService::new(redis.store().await, driver, config(redis.url.clone()));
    let app = router(service);
    let body = r#"{"idempotency_key":"order-123","recipient":"0x00000000000000000000000000000000000000A1","amount":"12500000","reason":"order_payout"}"#;

    let unauthorized = app
        .clone()
        .oneshot(
            Request::post("/api/internal/transfers")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let disabled_erc20 = app
        .clone()
        .oneshot(
            Request::post("/api/internal/erc20-usdc/transfers")
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::AUTHORIZATION,
                    "Bearer a-secure-machine-token-with-32-characters",
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disabled_erc20.status(), StatusCode::NOT_FOUND);

    let authorized = app
        .clone()
        .oneshot(
            Request::post("/api/internal/transfers")
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::AUTHORIZATION,
                    "Bearer a-secure-machine-token-with-32-characters",
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);
    let response: serde_json::Value =
        serde_json::from_slice(&authorized.into_body().collect().await.unwrap().to_bytes())
            .unwrap();
    assert_eq!(response["idempotency_key"], "order-123");
    assert_eq!(response["amount"], "12500000");
    assert_eq!(response["replayed"], false);

    let liveness = app
        .clone()
        .oneshot(
            Request::get("/api/internal/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(liveness.status(), StatusCode::OK);
    let readiness = app
        .clone()
        .oneshot(
            Request::get("/api/internal/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(readiness.status(), StatusCode::UNAUTHORIZED);
    let authenticated_readiness = app
        .oneshot(
            Request::get("/api/internal/readiness")
                .header(
                    header::AUTHORIZATION,
                    "Bearer a-secure-machine-token-with-32-characters",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authenticated_readiness.status(), StatusCode::OK);
}

#[tokio::test]
async fn erc20_usdc_route_preserves_machine_funding_wire_shape() {
    let redis = TestRedis::start().await;
    let legacy = FundingService::new(
        redis.store().await,
        FakeDriver::new([]),
        config(redis.url.clone()),
    );
    let mut erc20_config = config(redis.url.clone());
    enable_erc20_usdc(&mut erc20_config);
    let driver = FakeDriver::new([ChainResult::Success])
        .with_operator_key("5124:0xerc20machine")
        .with_deployment_identity(erc20_identity(&erc20_config));
    let erc20 = FundingService::new(redis.store().await, driver, erc20_config);
    let app = router_with_erc20(legacy, Erc20FundingService::Enabled(Box::new(erc20)));
    let body = r#"{"idempotency_key":"erc20-order-123","recipient":"0x00000000000000000000000000000000000000A1","amount":"12500000","reason":"order_payout"}"#;

    let response = app
        .oneshot(
            Request::post("/api/internal/erc20-usdc/transfers")
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::AUTHORIZATION,
                    "Bearer a-secure-machine-token-with-32-characters",
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["idempotency_key"], "erc20-order-123");
    assert_eq!(body["amount"], "12500000");
    assert_eq!(body["replayed"], false);
}
