use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::config::ResolvedConfig;

#[derive(Args, Debug, Default)]
pub struct StatusCommand {}

impl StatusCommand {
    pub async fn execute(&self, config: &ResolvedConfig) -> Result<()> {
        let client = config.client();

        // Query the live node; if unreachable, still show config with a clear error.
        let node = match client.get_health().await {
            Ok(health) => Some(LiveNodeStatus {
                status: health.status,
                version: health.version,
                latest_slot: health.latest_slot,
                finalized_slot: health.finalized_slot,
                peer_count: health.peer_count,
                syncing: health
                    .sync
                    .get("syncing")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            }),
            Err(e) => {
                eprintln!("warning: could not reach node at {}: {e}", config.endpoint);
                None
            }
        };

        let summary = StatusSummary {
            endpoint: config.endpoint.clone(),
            default_key: config
                .default_key_path()
                .map(|path| path.display().to_string()),
            default_fee: config.client_config.default_fee,
            default_gas_limit: config.client_config.default_gas_limit,
            node,
        };

        println!("{}", serde_json::to_string_pretty(&summary)?);
        Ok(())
    }
}

/// Live node status fetched from `aeth_health`.
#[derive(Serialize)]
struct LiveNodeStatus {
    status: String,
    version: String,
    latest_slot: u64,
    finalized_slot: u64,
    peer_count: usize,
    syncing: bool,
}

#[derive(Serialize)]
struct StatusSummary {
    endpoint: String,
    default_key: Option<String>,
    default_fee: u128,
    default_gas_limit: u64,
    /// Present when the node is reachable; absent when it is offline.
    #[serde(skip_serializing_if = "Option::is_none")]
    node: Option<LiveNodeStatus>,
}
