use std::fs;
use std::path::{Path, PathBuf};

use aether_sdk::types::ClientConfig;
use aether_sdk::AetherClient;
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

#[derive(Debug)]
pub struct ResolvedConfig {
    pub endpoint: String,
    pub client_config: ClientConfig,
    pub default_key: Option<PathBuf>,
}

impl ResolvedConfig {
    pub fn client(&self) -> AetherClient {
        AetherClient::with_config(self.endpoint.clone(), self.client_config.clone())
    }

    pub fn default_key_path(&self) -> Option<&Path> {
        self.default_key.as_deref()
    }
}

#[derive(Debug, Deserialize, Default)]
struct RawConfig {
    endpoint: Option<String>,
    default_key: Option<String>,
    fee: Option<u128>,
    gas_limit: Option<u64>,
}

pub fn load_config(path: Option<&str>) -> Result<ResolvedConfig> {
    let config_path = resolve_config_path(path)?;
    let raw = if config_path.exists() {
        let contents = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read config file {}", config_path.display()))?;
        toml::from_str::<RawConfig>(&contents)
            .with_context(|| format!("failed to parse config file {}", config_path.display()))?
    } else {
        RawConfig::default()
    };

    let endpoint = raw
        .endpoint
        .unwrap_or_else(|| "http://localhost:8545".to_string());

    let mut client_config = ClientConfig::default();
    if let Some(fee) = raw.fee {
        client_config.default_fee = fee;
    }
    if let Some(gas) = raw.gas_limit {
        client_config.default_gas_limit = gas;
    }

    let default_key = match raw.default_key {
        Some(path) => Some(expand_path(&path)?),
        None => None,
    };

    Ok(ResolvedConfig {
        endpoint,
        client_config,
        default_key,
    })
}

fn resolve_config_path(path: Option<&str>) -> Result<PathBuf> {
    if let Some(custom) = path {
        return expand_path(custom);
    }
    if let Some(home) = dirs::home_dir() {
        Ok(home.join(".aether").join("config.toml"))
    } else {
        Err(anyhow!(
            "unable to resolve home directory; pass --config explicitly"
        ))
    }
}

pub fn expand_path(path: &str) -> Result<PathBuf> {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return Ok(home.join(stripped));
        } else {
            return Err(anyhow!(
                "unable to resolve home directory for path {}",
                path
            ));
        }
    }
    Ok(Path::new(path).to_path_buf())
}
