use std::env;
use std::net::SocketAddr;
use std::time::Duration;

use aether_faucet::{faucet_app, FaucetConfig};
use axum::serve;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let mut config = FaucetConfig::default();
    if let Ok(limit) = env::var("AETHER_FAUCET_LIMIT") {
        match limit.parse() {
            Ok(parsed) => config.default_amount_limit = parsed,
            Err(e) => tracing::warn!(
                "ignoring invalid AETHER_FAUCET_LIMIT={:?}: {}; using default {}",
                limit,
                e,
                config.default_amount_limit
            ),
        }
    }
    if let Ok(cooldown) = env::var("AETHER_FAUCET_COOLDOWN") {
        match cooldown.parse::<u64>() {
            Ok(parsed) => config.cooldown = Duration::from_secs(parsed),
            Err(e) => tracing::warn!(
                "ignoring invalid AETHER_FAUCET_COOLDOWN={:?}: {}; using default {}s",
                cooldown,
                e,
                config.cooldown.as_secs()
            ),
        }
    }

    let app = faucet_app(config);

    let addr: SocketAddr = env::var("AETHER_FAUCET_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()?;
    info!(%addr, "starting faucet listener");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    serve(listener, app.into_make_service()).await?;

    Ok(())
}
