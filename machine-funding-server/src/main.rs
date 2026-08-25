use faucet_machine_funding::store::FundingStore;
use faucet_machine_funding::{router, Config, EvmChainDriver, FundingService, RedisStore};
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
    let driver = EvmChainDriver::connect(&config).await?;
    let bind_addr = config.bind_addr;
    let service = FundingService::new(store, driver, config);
    service.health().await?;
    let listener = TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "machine funding service listening");
    axum::serve(listener, router(service))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async { signal::ctrl_c().await.expect("install Ctrl-C handler") };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("machine funding service shutting down");
}
