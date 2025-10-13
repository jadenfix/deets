use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::config::ResolvedConfig;

#[derive(Args, Debug, Default)]
pub struct StatusCommand {}

impl StatusCommand {
    pub async fn execute(&self, config: &ResolvedConfig) -> Result<()> {
        let summary = StatusSummary {
            endpoint: config.endpoint.clone(),
            default_key: config
                .default_key_path()
                .map(|path| path.display().to_string()),
            default_fee: config.client_config.default_fee,
            default_gas_limit: config.client_config.default_gas_limit,
        };

        println!("{}", serde_json::to_string_pretty(&summary)?);
        Ok(())
    }
}

#[derive(Serialize)]
struct StatusSummary {
    endpoint: String,
    default_key: Option<String>,
    default_fee: u128,
    default_gas_limit: u64,
}
