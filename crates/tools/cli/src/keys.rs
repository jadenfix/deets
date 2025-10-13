use std::path::PathBuf;

use aether_crypto_primitives::Keypair;
use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::config::{expand_path, ResolvedConfig};
use crate::io::{address_to_string, read_key_file, write_key_file};

#[derive(Subcommand, Debug)]
pub enum KeyCommands {
    /// Generate a new Ed25519 keypair and write it to disk
    Generate(GenerateArgs),
    /// Show information about an existing keypair
    Show(ShowArgs),
}

impl KeyCommands {
    pub async fn execute(&self, config: &ResolvedConfig) -> Result<()> {
        match self {
            KeyCommands::Generate(args) => args.execute().await,
            KeyCommands::Show(args) => args.execute(config).await,
        }
    }
}

#[derive(Args, Debug)]
pub struct GenerateArgs {
    /// Output path for the generated key file
    #[arg(long)]
    pub out: String,

    /// Overwrite an existing file if present
    #[arg(long, default_value_t = false)]
    pub overwrite: bool,
}

impl GenerateArgs {
    pub async fn execute(&self) -> Result<()> {
        let path = expand_path(&self.out)?;
        if path.exists() && !self.overwrite {
            return Err(anyhow!(
                "key file {} already exists (use --overwrite to replace)",
                path.display()
            ));
        }

        let keypair = Keypair::generate();
        write_key_file(&path, &keypair)?;
        let public_key = format!("0x{}", hex::encode(keypair.public_key()));
        let address_bytes = keypair.to_address();
        let address = format!("0x{}", hex::encode(address_bytes));

        let summary = KeySummary {
            path,
            public_key,
            address,
        };
        println!("{}", serde_json::to_string_pretty(&summary)?);
        Ok(())
    }
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Path to the key file (defaults to config default key)
    #[arg(long)]
    pub path: Option<String>,
}

impl ShowArgs {
    pub async fn execute(&self, config: &ResolvedConfig) -> Result<()> {
        let path = if let Some(path) = &self.path {
            expand_path(path)?
        } else if let Some(default) = config.default_key_path() {
            default.to_path_buf()
        } else {
            return Err(anyhow!(
                "no key path provided and default_key is not configured"
            ));
        };

        let key = read_key_file(&path)?;
        let summary = KeySummary {
            path,
            public_key: format!("0x{}", hex::encode(key.keypair.public_key())),
            address: address_to_string(&key.address),
        };
        println!("{}", serde_json::to_string_pretty(&summary)?);
        Ok(())
    }
}

#[derive(Serialize)]
struct KeySummary {
    path: PathBuf,
    public_key: String,
    address: String,
}
