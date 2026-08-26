use alloy_primitives::{Address, U256};
use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use faucet_machine_funding::{
    chain::ChainDriver,
    model::{ChainResult, FundingAsset, FundingInput, PreparedTransaction, ServiceError},
    router,
    store::FundingStore,
    Config, FundingService, RedisStore,
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
        }
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
        "5124:0xmachine"
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
        gas_amount_wei: U256::from_str_radix("10000000000000000", 10).unwrap(),
        global_susdc_budget: U256::from(1_000_000_000u64),
        global_gas_budget_wei: U256::from_str_radix("1000000000000000000", 10).unwrap(),
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
        idempotency_key: key.into(),
        recipient: Address::from_str(RECIPIENT).unwrap(),
        recipient_text: RECIPIENT.into(),
        amount: U256::from(amount),
        reason: "order_payout".into(),
    }
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
