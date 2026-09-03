use faucet_machine_funding::store::FundingStore;
use faucet_machine_funding::{
    router_with_networks, BaseActivation, BaseChainDriver, BaseFundingService, ChainDriver, Config,
    Erc20FundingService, Erc20UsdcActivation, EvmChainDriver, FundingService, RedisStore,
};
use std::{env, io};
use tokio::{net::TcpListener, signal};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let config = Config::from_env()?;
    let store = RedisStore::connect(&config.redis_url).await?;
    store.verify_durability().await?;
    if env::args().any(|argument| argument == "--check-erc20-usdc") {
        return check_erc20_usdc(&config, &store).await;
    }
    if env::args().any(|argument| argument == "--check-base") {
        return check_base(&config, &store).await;
    }
    let driver = EvmChainDriver::connect(&config).await?;
    let bind_addr = config.bind_addr;
    let legacy = FundingService::new(store.clone(), driver, config.clone());
    legacy.health().await?;
    let erc20_usdc = erc20_service(&config, store.clone()).await;
    let base = base_service(&config, store).await;
    let listener = TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "machine funding service listening");
    axum::serve(listener, router_with_networks(legacy, erc20_usdc, base))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Startup preflight for Base: chain id, signer/address match, native ETH
/// reserve, token code, six decimals, and the ERC20 reserve.
async fn check_base(config: &Config, store: &RedisStore) -> Result<(), Box<dyn std::error::Error>> {
    let base = config.base.enabled().ok_or_else(|| {
        io::Error::other("valid enabled Base configuration is required for preflight")
    })?;
    store.ping().await?;
    let driver = BaseChainDriver::connect(base, config.receipt_timeout).await?;
    let reserves = driver.reserves().await?;
    tracing::info!(
        native_balance = %reserves.native_balance,
        erc20_usdc_balance = %reserves.erc20_usdc_balance,
        "Base funding preflight passed"
    );
    Ok(())
}

async fn base_service(
    config: &Config,
    store: RedisStore,
) -> BaseFundingService<RedisStore, BaseChainDriver> {
    match &config.base {
        BaseActivation::Disabled => BaseFundingService::Disabled,
        BaseActivation::Invalid(error) => {
            tracing::error!(%error, "Base funding configuration is invalid");
            BaseFundingService::Unavailable
        }
        BaseActivation::Enabled(base) => {
            match BaseChainDriver::connect(base, config.receipt_timeout).await {
                Ok(driver) => BaseFundingService::Enabled(Box::new(FundingService::new(
                    store,
                    driver,
                    config.clone(),
                ))),
                Err(error) => {
                    tracing::error!(%error, "Base funding preflight failed");
                    BaseFundingService::Unavailable
                }
            }
        }
    }
}

async fn check_erc20_usdc(
    config: &Config,
    store: &RedisStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let erc20_usdc = config.erc20_usdc.enabled().ok_or_else(|| {
        io::Error::other("valid enabled ERC20 USDC configuration is required for preflight")
    })?;
    store.ping().await?;
    let driver = EvmChainDriver::connect_erc20_usdc(config, erc20_usdc).await?;
    driver.health().await?;
    tracing::info!("ERC20 USDC funding preflight passed");
    Ok(())
}

async fn erc20_service(
    config: &Config,
    store: RedisStore,
) -> Erc20FundingService<RedisStore, EvmChainDriver> {
    match &config.erc20_usdc {
        Erc20UsdcActivation::Disabled => Erc20FundingService::Disabled,
        Erc20UsdcActivation::Invalid(error) => {
            tracing::error!(%error, "ERC20 USDC funding configuration is invalid");
            Erc20FundingService::Unavailable
        }
        Erc20UsdcActivation::Enabled(erc20_usdc) => {
            match EvmChainDriver::connect_erc20_usdc(config, erc20_usdc).await {
                Ok(driver) => Erc20FundingService::Enabled(Box::new(FundingService::new(
                    store,
                    driver,
                    config.clone(),
                ))),
                Err(error) => {
                    tracing::error!(%error, "ERC20 USDC funding preflight failed");
                    Erc20FundingService::Unavailable
                }
            }
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = signal::ctrl_c().await {
            tracing::error!(%error, "failed to install Ctrl-C handler");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::error!(%error, "failed to install SIGTERM handler"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("machine funding service shutting down");
}
