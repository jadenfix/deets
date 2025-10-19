use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::ResolvedConfig;
use crate::io::{address_to_string, ensure_parent_dir, parse_address};
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use serde::Serialize;

const DEFAULT_VALIDATOR_STAKE: u128 = 1_000_000;

#[derive(Debug, Serialize)]
struct GenesisValidator {
    address: String,
    stake: u128,
}

#[derive(Debug, Serialize)]
struct GenesisParameters {
    epoch_length: u64,
    slot_duration_ms: u64,
    tau: f64,
}

#[derive(Debug, Serialize)]
struct GenesisFile {
    chain_id: String,
    timestamp: u64,
    initial_supply: u128,
    validators: Vec<GenesisValidator>,
    parameters: GenesisParameters,
}

#[derive(Parser, Debug)]
pub struct InitGenesisCommand {
    /// Path to output genesis file
    #[arg(long, default_value = "genesis.json")]
    output: String,

    /// Initial validators (comma-separated address or address:stake entries)
    #[arg(long)]
    validators: Option<String>,

    /// Initial token supply
    #[arg(long, default_value = "1000000000")]
    supply: u128,
}

impl InitGenesisCommand {
    pub async fn execute(&self, _config: &ResolvedConfig) -> Result<()> {
        let validators = self.parse_validators()?;
        let total_stake: u128 = validators.iter().map(|v| v.stake).sum();
        if total_stake > self.supply {
            return Err(anyhow!(
                "total validator stake {} exceeds initial supply {}",
                total_stake,
                self.supply
            ));
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let genesis = GenesisFile {
            chain_id: "aether-devnet".to_string(),
            timestamp,
            initial_supply: self.supply,
            validators,
            parameters: GenesisParameters {
                epoch_length: 720,
                slot_duration_ms: 500,
                tau: 0.8,
            },
        };

        let output_path = Path::new(&self.output);
        ensure_parent_dir(output_path)?;
        let payload = serde_json::to_vec_pretty(&genesis)?;
        fs::write(output_path, payload)
            .with_context(|| format!("failed to write genesis file {}", output_path.display()))?;

        println!(
            "Genesis written to {} (validators: {}, initial_supply: {})",
            output_path.display(),
            genesis.validators.len(),
            genesis.initial_supply
        );
        Ok(())
    }

    fn parse_validators(&self) -> Result<Vec<GenesisValidator>> {
        let Some(raw) = &self.validators else {
            return Ok(Vec::new());
        };

        let mut parsed = Vec::new();
        let mut seen = HashSet::new();

        for entry in raw.split(',').filter(|s| !s.trim().is_empty()) {
            let parts: Vec<_> = entry.split(':').collect();
            if parts.is_empty() || parts.len() > 2 {
                return Err(anyhow!(
                    "invalid validator entry '{}'; expected address or address:stake",
                    entry
                ));
            }

            let address = parse_address(parts[0].trim())?;
            let stake = if parts.len() == 2 {
                parts[1].trim().parse::<u128>().map_err(|_| {
                    anyhow!(
                        "invalid stake for validator '{}': expected unsigned integer",
                        entry
                    )
                })?
            } else {
                DEFAULT_VALIDATOR_STAKE
            };

            let formatted = address_to_string(&address);
            if !seen.insert(formatted.clone()) {
                return Err(anyhow!(
                    "duplicate validator address {} in validator list",
                    formatted
                ));
            }

            parsed.push(GenesisValidator {
                address: formatted,
                stake,
            });
        }

        Ok(parsed)
    }
}
