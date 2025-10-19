use std::fs;
use std::path::PathBuf;

use crate::config::{expand_path, ResolvedConfig};
use crate::io::{address_to_string, read_key_file};
use aether_consensus::SimpleConsensus;
use aether_node::Node;
use aether_types::{PublicKey, ValidatorInfo};
use anyhow::{Context, Result};
use clap::Parser;

const DEFAULT_VALIDATOR_STAKE: u128 = 1_000_000;

#[derive(Parser, Debug)]
pub struct RunCommand {
    /// Path to data directory
    #[arg(long, default_value = "~/.aether/data")]
    data_dir: String,

    /// Path to validator key (generated via `aetherctl keys generate`)
    #[arg(long)]
    validator_key: Option<String>,

    /// RPC port to bind
    #[arg(long, default_value = "8545")]
    rpc_port: u16,

    /// P2P port to bind
    #[arg(long, default_value = "9000")]
    p2p_port: u16,

    /// Bootstrap peers (comma-separated multiaddr list)
    #[arg(long)]
    bootstrap: Option<String>,
}

impl RunCommand {
    pub async fn execute(&self, _config: &ResolvedConfig) -> Result<()> {
        let data_dir = expand_path(&self.data_dir)?;
        ensure_dir(&data_dir)?;

        let key_material = if let Some(path) = &self.validator_key {
            let expanded = expand_path(path)?;
            Some(read_key_file(&expanded)?)
        } else {
            None
        };

        let (validators, validator_key, validator_address) = if let Some(material) = key_material {
            let pubkey = PublicKey::from_bytes(material.keypair.public_key());
            let info = ValidatorInfo {
                pubkey,
                stake: DEFAULT_VALIDATOR_STAKE,
                commission: 0,
                active: true,
            };
            (
                vec![info],
                Some(material.keypair),
                Some(address_to_string(&material.address)),
            )
        } else {
            (Vec::new(), None, None)
        };

        let consensus = SimpleConsensus::new(validators);
        let ledger_path = data_dir.join("ledger");
        ensure_dir(&ledger_path)?;

        let mut node = Node::new(ledger_path, Box::new(consensus), validator_key)
            .context("failed to initialize node")?;

        println!("Starting Aether node");
        println!("  data directory : {}", data_dir.display());
        println!("  rpc endpoint   : http://127.0.0.1:{}", self.rpc_port);
        println!("  p2p port       : {}", self.p2p_port);

        if let Some(peers) = &self.bootstrap {
            let peers: Vec<_> = peers
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            if !peers.is_empty() {
                println!("  bootstrap peers: {}", peers.join(", "));
            }
        }

        if let Some(address) = validator_address {
            println!("  role           : validator");
            println!("  validator addr : {}", address);
        } else {
            println!("  role           : full node");
        }

        node.run().await.context("node execution failed")
    }
}

fn ensure_dir(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)
            .with_context(|| format!("failed to create directory {}", path.display()))?;
    }
    Ok(())
}
