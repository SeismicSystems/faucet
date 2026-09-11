use alloy_primitives::{Address, U256};
use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use faucet_machine_funding::{
    chain::ChainDriver,
    config::{BaseActivation, BaseConfig, Erc20UsdcActivation, Erc20UsdcConfig},
    model::{
        BaseNetworkIdentity, ChainResult, Erc20DeploymentIdentity, FundingAsset, FundingInput,
        FundingRecord, PersistedInput, PreparedTransaction, ReserveDiagnostic, ServiceError,
    },
    router, router_with_erc20, router_with_networks,
    store::FundingStore,
    BaseFundingService, Config, Erc20FundingService, FundingService, RedisStore,
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
    network_identity: Option<BaseNetworkIdentity>,
    reserves: Option<ReserveDiagnostic>,
    retry_legacy: bool,
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
            network_identity: None,
            reserves: None,
            retry_legacy: false,
        }
    }

    fn with_network_identity(mut self, identity: BaseNetworkIdentity) -> Self {
        self.network_identity = Some(identity);
        self
    }

    fn with_reserves(mut self, reserves: ReserveDiagnostic) -> Self {
        self.reserves = Some(reserves);
        self
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
    async fn can_retry_reverted(
        &self,
        input: &FundingInput,
        transaction: &PreparedTransaction,
    ) -> Result<bool, ServiceError> {
        self.validate_input(input)?;
        Ok(self.retry_legacy && input.asset == FundingAsset::BaseEth && transaction.nonce == 0)
    }
    fn operator_key(&self) -> &str {
        &self.operator_key
    }

    fn validate_input(&self, input: &FundingInput) -> Result<(), ServiceError> {
        let mismatch = || {
            ServiceError::new(
                409,
                "deployment_identity_mismatch",
                "Funding request belongs to a different contract deployment",
            )
        };
        if let Some(identity) = &self.network_identity {
            return if input.asset.is_base() && input.network_identity.as_ref() == Some(identity) {
                Ok(())
            } else {
                Err(mismatch())
            };
        }
        if input.network_identity.is_some() || input.asset.is_base() {
            return Err(mismatch());
        }
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
            _ => Err(mismatch()),
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

    async fn reserve_diagnostic(&self) -> Result<Option<ReserveDiagnostic>, ServiceError> {
        Ok(self.reserves.clone())
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
        base: BaseActivation::Disabled,
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
        network_identity: None,
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

const BASE_TOKEN: &str = "0x036CbD53842c5426634e7929541eC2318f3dCF7e";
const BASE_RESERVE: &str = "0x5A0b54D5dc17e0AadC383d2db43B0a0D3E029c4c";
const BASE_GAS_WEI: u64 = 1_000_000_000_000_000;

fn base_config() -> BaseConfig {
    BaseConfig {
        rpc_url: "http://127.0.0.1:8546".into(),
        chain_id: 84532,
        private_key: format!("0x{}", "3".repeat(64)),
        reserve_address: Address::from_str(BASE_RESERVE).unwrap(),
        token_address: Address::from_str(BASE_TOKEN).unwrap(),
        gas_eth_amount: U256::from(BASE_GAS_WEI),
        max_erc20_usdc_amount: U256::from(250_000_000u64),
        global_gas_eth_budget: U256::from(100_000_000_000_000_000u64),
        global_erc20_usdc_budget: U256::from(1_000_000_000u64),
        eth_reserve_floor: U256::from(BASE_GAS_WEI),
        erc20_usdc_reserve_floor: U256::from(250_000_000u64),
        rate_limit: 10,
        rate_window: Duration::from_secs(60),
        confirmations: 1,
    }
}

fn enable_base(config: &mut Config) {
    config.base = BaseActivation::Enabled(Box::new(base_config()));
}

fn base_identity(config: &Config) -> BaseNetworkIdentity {
    config.base.enabled().unwrap().identity()
}

fn base_request(config: &Config, asset: FundingAsset, key: &str, amount: u64) -> FundingInput {
    let mut input = request(key, amount);
    input.asset = asset;
    input.network_identity = Some(base_identity(config));
    input
}

fn base_driver(config: &Config, results: impl IntoIterator<Item = ChainResult>) -> FakeDriver {
    FakeDriver::new(results)
        .with_operator_key("84532:0xbasereserve")
        .with_network_identity(base_identity(config))
}

fn healthy_reserves() -> ReserveDiagnostic {
    ReserveDiagnostic {
        chain_id: 84532,
        reserve_address: BASE_RESERVE.into(),
        native_balance: "50000000000000000".into(),
        native_floor: BASE_GAS_WEI.to_string(),
        native_low: false,
        erc20_usdc_balance: "1000000000".into(),
        erc20_usdc_floor: "250000000".into(),
        erc20_usdc_low: false,
    }
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

#[tokio::test]
async fn base_records_are_scoped_to_asset_and_network_identity() {
    let redis = TestRedis::start().await;
    let legacy_driver = FakeDriver::new([ChainResult::Success]);
    let legacy_service = FundingService::new(
        redis.store().await,
        legacy_driver.clone(),
        config(redis.url.clone()),
    );
    let mut base_config = config(redis.url.clone());
    enable_base(&mut base_config);
    let identity = base_identity(&base_config);
    let driver = base_driver(&base_config, [ChainResult::Success, ChainResult::Success]);
    let base_service =
        FundingService::new(redis.store().await, driver.clone(), base_config.clone());

    legacy_service
        .execute(request("shared-key", 12_500_000))
        .await
        .unwrap();
    let gas = base_service
        .execute(base_request(
            &base_config,
            FundingAsset::BaseEth,
            "shared-key",
            BASE_GAS_WEI,
        ))
        .await
        .unwrap();
    let usdc = base_service
        .execute(base_request(
            &base_config,
            FundingAsset::BaseErc20Usdc,
            "shared-key",
            12_500_000,
        ))
        .await
        .unwrap();
    assert_ne!(gas.transaction_hash, usdc.transaction_hash);
    assert!(!gas.replayed && !usdc.replayed);

    let mut connection = redis.raw_connection().await;
    for key in [
        "machine-funding:idempotency:shared-key".to_owned(),
        format!(
            "machine-funding:base_eth:{}:idempotency:shared-key",
            identity.scope()
        ),
        format!(
            "machine-funding:base_erc20_usdc:{}:idempotency:shared-key",
            identity.scope()
        ),
    ] {
        let record: Option<String> = connection.get(&key).await.unwrap();
        assert!(record.is_some(), "{key} should exist");
    }
    assert_eq!(legacy_driver.counts(), (1, 1));
    assert_eq!(driver.counts(), (2, 2));
}

#[tokio::test]
async fn base_gas_replays_identically_and_never_double_funds() {
    let redis = TestRedis::start().await;
    let mut base_config = config(redis.url.clone());
    enable_base(&mut base_config);
    let driver = base_driver(&base_config, [ChainResult::Success]);
    let service = FundingService::new(redis.store().await, driver.clone(), base_config.clone());
    let gas = base_request(
        &base_config,
        FundingAsset::BaseEth,
        "wallet-1",
        BASE_GAS_WEI,
    );

    let (first, second) = tokio::join!(service.execute(gas.clone()), service.execute(gas.clone()));
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.transaction_hash, second.transaction_hash);
    assert_ne!(first.replayed, second.replayed);
    let third = service.execute(gas).await.unwrap();
    assert!(third.replayed);
    assert_eq!(third.amount, BASE_GAS_WEI.to_string());
    assert_eq!(driver.counts(), (1, 1));
}

#[tokio::test]
async fn legacy_base_gas_retry_preserves_history_budget_and_concurrent_idempotency() {
    let redis = TestRedis::start().await;
    let mut config = config(redis.url.clone());
    enable_base(&mut config);
    let BaseActivation::Enabled(limits) = &mut config.base else {
        unreachable!()
    };
    limits.rate_limit = 1;
    limits.global_gas_eth_budget = U256::from(BASE_GAS_WEI);
    let mut driver = base_driver(
        &config,
        [
            ChainResult::Reverted,
            ChainResult::Pending,
            ChainResult::Success,
        ],
    );
    driver.retry_legacy = true;
    let gas = base_request(&config, FundingAsset::BaseEth, "legacy-retry", BASE_GAS_WEI);
    let store = redis.store().await;
    let service = FundingService::new(store.clone(), driver.clone(), config.clone());
    assert_eq!(
        service.execute(gas.clone()).await.unwrap_err().code,
        "transaction_reverted"
    );
    let key = format!(
        "machine-funding:base_eth:{}:idempotency:legacy-retry",
        gas.network_identity.as_ref().unwrap().scope()
    );
    let failed = store.get_record(&key).await.unwrap().unwrap();
    let FundingRecord::Failed { transaction, .. } = &failed else {
        panic!("expected failed record")
    };
    assert_eq!(
        service.execute(gas.clone()).await.unwrap_err().code,
        "transaction_pending"
    );
    let archived = store
        .get_record(&format!("{key}:reverted:{}", transaction.hash))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(archived, FundingRecord::Failed { .. }));
    assert!(store
        .retry_reverted(&key, "unused-queue", &failed)
        .await
        .is_err());
    let restarted = FundingService::new(store, driver.clone(), config);
    let (first, second) = tokio::join!(
        restarted.execute(gas.clone()),
        restarted.execute(gas.clone())
    );
    assert_eq!(
        first.unwrap().transaction_hash,
        second.unwrap().transaction_hash
    );
    assert!(restarted.execute(gas).await.unwrap().replayed);
    assert_eq!(driver.counts(), (2, 3));
}

#[tokio::test]
async fn nonrecoverable_base_reverts_remain_terminal() {
    let redis = TestRedis::start().await;
    let mut config = config(redis.url.clone());
    enable_base(&mut config);
    let driver = base_driver(&config, [ChainResult::Reverted]);
    let service = FundingService::new(redis.store().await, driver.clone(), config.clone());
    let gas = base_request(&config, FundingAsset::BaseEth, "no-retry", BASE_GAS_WEI);
    for _ in 0..2 {
        assert_eq!(
            service.execute(gas.clone()).await.unwrap_err().code,
            "transaction_reverted"
        );
    }
    assert_eq!(driver.counts(), (1, 1));
}

#[tokio::test]
async fn base_budgets_and_rate_limits_are_independent_from_seismic() {
    let redis = TestRedis::start().await;
    let mut base_config = config(redis.url.clone());
    enable_base(&mut base_config);
    let BaseActivation::Enabled(limits) = &mut base_config.base else {
        unreachable!();
    };
    limits.global_erc20_usdc_budget = U256::from(20u64);
    limits.global_gas_eth_budget = U256::from(BASE_GAS_WEI);
    limits.rate_limit = 1;
    let driver = base_driver(&base_config, [ChainResult::Success, ChainResult::Success]);
    let service = FundingService::new(redis.store().await, driver.clone(), base_config.clone());

    service
        .execute(base_request(
            &base_config,
            FundingAsset::BaseErc20Usdc,
            "usdc-1",
            15,
        ))
        .await
        .unwrap();
    let rate_error = service
        .execute(base_request(
            &base_config,
            FundingAsset::BaseErc20Usdc,
            "usdc-2",
            1,
        ))
        .await
        .unwrap_err();
    assert_eq!(rate_error.code, "rate_limited");
    let mut other = base_request(&base_config, FundingAsset::BaseErc20Usdc, "usdc-3", 10);
    other.recipient = Address::with_last_byte(0xa2);
    other.recipient_text = other.recipient.to_checksum(None);
    assert_eq!(
        service.execute(other).await.unwrap_err().code,
        "global_budget_exceeded"
    );

    let mut gas = base_request(&base_config, FundingAsset::BaseEth, "gas-1", BASE_GAS_WEI);
    gas.recipient = Address::with_last_byte(0xa3);
    gas.recipient_text = gas.recipient.to_checksum(None);
    service.execute(gas).await.unwrap();
    let mut depleted = base_request(&base_config, FundingAsset::BaseEth, "gas-2", BASE_GAS_WEI);
    depleted.recipient = Address::with_last_byte(0xa4);
    depleted.recipient_text = depleted.recipient.to_checksum(None);
    assert_eq!(
        service.execute(depleted).await.unwrap_err().code,
        "global_budget_exceeded"
    );

    let legacy_driver = FakeDriver::new([ChainResult::Success]);
    let legacy_service = FundingService::new(
        redis.store().await,
        legacy_driver.clone(),
        config(redis.url.clone()),
    );
    legacy_service
        .execute(request("legacy-1", 10))
        .await
        .unwrap();
    assert_eq!(legacy_driver.counts(), (1, 1));
    assert_eq!(driver.counts(), (2, 2));
}

#[tokio::test]
async fn base_depleted_reserve_is_terminal_and_never_rebroadcast() {
    let redis = TestRedis::start().await;
    let mut base_config = config(redis.url.clone());
    enable_base(&mut base_config);
    let driver = base_driver(
        &base_config,
        [
            ChainResult::Rejected("insufficient funds for gas * price + value".into()),
            ChainResult::Success,
        ],
    );
    let service = FundingService::new(redis.store().await, driver.clone(), base_config.clone());
    let gas = base_request(
        &base_config,
        FundingAsset::BaseEth,
        "gas-depleted",
        BASE_GAS_WEI,
    );
    let first = service.execute(gas.clone()).await.unwrap_err();
    let replay = service.execute(gas).await.unwrap_err();
    assert_eq!(first.status, 422);
    assert_eq!(first.code, "transaction_rejected");
    assert_eq!(replay.code, "transaction_rejected");
    assert_eq!(driver.counts(), (1, 1));
}

#[tokio::test]
async fn base_requests_are_refused_by_a_seismic_driver_and_vice_versa() {
    let redis = TestRedis::start().await;
    let mut base_config = config(redis.url.clone());
    enable_base(&mut base_config);
    let seismic = FundingService::new(
        redis.store().await,
        FakeDriver::new([ChainResult::Success]),
        base_config.clone(),
    );
    let wrong_driver = seismic
        .execute(base_request(
            &base_config,
            FundingAsset::BaseEth,
            "cross-1",
            BASE_GAS_WEI,
        ))
        .await
        .unwrap_err();
    assert_eq!(wrong_driver.code, "deployment_identity_mismatch");

    let base = FundingService::new(
        redis.store().await,
        base_driver(&base_config, [ChainResult::Success]),
        base_config.clone(),
    );
    let wrong_asset = base.execute(request("cross-2", 10)).await.unwrap_err();
    assert_eq!(wrong_asset.code, "deployment_identity_mismatch");

    let mut rotated = base_request(&base_config, FundingAsset::BaseErc20Usdc, "cross-3", 10);
    rotated.network_identity = Some(BaseNetworkIdentity {
        token_address: Address::with_last_byte(0x11).to_checksum(None),
        ..base_identity(&base_config)
    });
    assert_eq!(
        base.execute(rotated).await.unwrap_err().code,
        "deployment_identity_mismatch"
    );
}

#[tokio::test]
async fn base_routes_are_disabled_by_default_and_require_auth() {
    let redis = TestRedis::start().await;
    let legacy = FundingService::new(
        redis.store().await,
        FakeDriver::new([]),
        config(redis.url.clone()),
    );
    let app = router_with_erc20(legacy, Erc20FundingService::Disabled);
    let body = r#"{"idempotency_key":"base-1","recipient":"0x00000000000000000000000000000000000000A1","reason":"wallet_registration"}"#;

    let unauthorized = app
        .clone()
        .oneshot(
            Request::post("/api/internal/base/gas")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    for path in [
        "/api/internal/base/gas",
        "/api/internal/base/erc20-usdc/transfers",
    ] {
        let disabled = app
            .clone()
            .oneshot(
                Request::post(path)
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
        assert_eq!(disabled.status(), StatusCode::NOT_FOUND, "{path}");
    }
    let readiness = app
        .oneshot(
            Request::get("/api/internal/base/readiness")
                .header(
                    header::AUTHORIZATION,
                    "Bearer a-secure-machine-token-with-32-characters",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(readiness.status(), StatusCode::NOT_FOUND);
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn settlement_identity_is_authenticated_and_uses_the_actual_token_holder() {
    let redis = TestRedis::start().await;
    let legacy = FundingService::new(
        redis.store().await,
        FakeDriver::new([]),
        config(redis.url.clone()),
    );
    let mut erc20_config = config(redis.url.clone());
    enable_erc20_usdc(&mut erc20_config);
    let expected = erc20_config.erc20_usdc.enabled().unwrap().faucet_address;
    let erc20 = FundingService::new(redis.store().await, FakeDriver::new([]), erc20_config);
    let app = router_with_erc20(legacy, Erc20FundingService::Enabled(Box::new(erc20)));
    let path = "/api/internal/erc20-usdc/settlement";
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .oneshot(
            Request::get(path)
                .header(
                    header::AUTHORIZATION,
                    "Bearer a-secure-machine-token-with-32-characters",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["treasury_address"], expected.to_string());
    assert_eq!(body.as_object().unwrap().len(), 3);
}

#[tokio::test]
async fn base_routes_preserve_the_machine_funding_wire_and_report_reserves() {
    let redis = TestRedis::start().await;
    let legacy = FundingService::new(
        redis.store().await,
        FakeDriver::new([]),
        config(redis.url.clone()),
    );
    let mut base_config = config(redis.url.clone());
    enable_base(&mut base_config);
    let driver = base_driver(&base_config, [ChainResult::Success, ChainResult::Success])
        .with_reserves(healthy_reserves());
    let base = FundingService::new(redis.store().await, driver, base_config);
    let app = router_with_networks(
        legacy,
        Erc20FundingService::Disabled,
        BaseFundingService::Enabled(Box::new(base)),
    );
    let auth = "Bearer a-secure-machine-token-with-32-characters";

    let identity = app
        .clone()
        .oneshot(
            Request::get("/api/internal/base/settlement")
                .header(header::AUTHORIZATION, auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(identity.status(), StatusCode::OK);
    let identity = json_body(identity).await;
    assert_eq!(identity["chain_id"], 84532);
    assert_eq!(
        identity["treasury_address"],
        healthy_reserves().reserve_address.to_lowercase()
    );

    let gas = app
        .clone()
        .oneshot(
            Request::post("/api/internal/base/gas")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, auth)
                .body(Body::from(
                    r#"{"idempotency_key":"base-gas-1","recipient":"0x00000000000000000000000000000000000000A1","reason":"wallet_registration"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gas.status(), StatusCode::OK);
    let gas = json_body(gas).await;
    assert_eq!(gas["idempotency_key"], "base-gas-1");
    assert_eq!(gas["amount"], BASE_GAS_WEI.to_string());
    assert_eq!(gas["replayed"], false);

    let over_limit = app
        .clone()
        .oneshot(
            Request::post("/api/internal/base/erc20-usdc/transfers")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, auth)
                .body(Body::from(
                    r#"{"idempotency_key":"base-usdc-big","recipient":"0x00000000000000000000000000000000000000A1","amount":"250000001","reason":"order_payout"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(over_limit.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(over_limit).await["error"]["code"],
        "amount_exceeds_limit"
    );

    let usdc = app
        .clone()
        .oneshot(
            Request::post("/api/internal/base/erc20-usdc/transfers")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, auth)
                .body(Body::from(
                    r#"{"idempotency_key":"base-usdc-1","recipient":"0x00000000000000000000000000000000000000A1","amount":"12500000","reason":"order_payout"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(usdc.status(), StatusCode::OK);
    let usdc = json_body(usdc).await;
    assert_eq!(usdc["amount"], "12500000");
    assert_eq!(
        usdc["recipient"],
        "0x00000000000000000000000000000000000000A1"
    );

    let readiness = app
        .oneshot(
            Request::get("/api/internal/base/readiness")
                .header(header::AUTHORIZATION, auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(readiness.status(), StatusCode::OK);
    let readiness = json_body(readiness).await;
    assert_eq!(readiness["status"], "ok");
    assert_eq!(readiness["reserves"]["chain_id"], 84532);
    assert_eq!(readiness["reserves"]["native_low"], false);
}

#[tokio::test]
async fn base_readiness_reports_a_low_reserve_as_unavailable() {
    let redis = TestRedis::start().await;
    let legacy = FundingService::new(
        redis.store().await,
        FakeDriver::new([]),
        config(redis.url.clone()),
    );
    let mut base_config = config(redis.url.clone());
    enable_base(&mut base_config);
    let mut low = healthy_reserves();
    low.native_balance = "1".into();
    low.native_low = true;
    let base = FundingService::new(
        redis.store().await,
        base_driver(&base_config, []).with_reserves(low),
        base_config,
    );
    let app = router_with_networks(
        legacy,
        Erc20FundingService::Disabled,
        BaseFundingService::Enabled(Box::new(base)),
    );
    let readiness = app
        .oneshot(
            Request::get("/api/internal/base/readiness")
                .header(
                    header::AUTHORIZATION,
                    "Bearer a-secure-machine-token-with-32-characters",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(readiness.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json_body(readiness).await["error"]["code"], "reserve_low");
}
