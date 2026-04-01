//! Genesis ceremony tool: generates validator keys and a shared genesis.json
//! for bootstrapping a multi-validator devnet.
//!
//! Usage:
//!   genesis-ceremony --validators 4 --output-dir /data/genesis
//!
//! Produces:
//!   /data/genesis/validator-1.key
//!   /data/genesis/validator-2.key
//!   ...
//!   /data/genesis/genesis.json

use aether_node::{GenesisConfig, ValidatorKeypair};
use aether_types::ChainConfig;
use anyhow::{Context, Result};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let mut num_validators: usize = 4;
    let mut output_dir = PathBuf::from(".");
    let mut stake_per_validator: u128 = 1_000_000;
    let mut network = "devnet".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--validators" | "-n" => {
                i += 1;
                num_validators = args[i].parse().context("invalid --validators value")?;
            }
            "--output-dir" | "-o" => {
                i += 1;
                output_dir = PathBuf::from(&args[i]);
            }
            "--stake" => {
                i += 1;
                stake_per_validator = args[i].parse().context("invalid --stake value")?;
            }
            "--network" => {
                i += 1;
                network = args[i].clone();
            }
            "--help" | "-h" => {
                eprintln!("Usage: genesis-ceremony [OPTIONS]");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  -n, --validators <N>    Number of validators (default: 4)");
                eprintln!("  -o, --output-dir <DIR>  Output directory (default: .)");
                eprintln!("  --stake <AMOUNT>        Stake per validator (default: 1000000)");
                eprintln!("  --network <NAME>        Network preset: devnet|testnet|mainnet (default: devnet)");
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create output dir: {}", output_dir.display()))?;

    let chain_config = match network.as_str() {
        "mainnet" => ChainConfig::mainnet(),
        "testnet" => ChainConfig::testnet(),
        _ => ChainConfig::devnet(),
    };

    println!("Generating {num_validators} validator keypairs...");
    let keypairs: Vec<ValidatorKeypair> =
        (0..num_validators).map(|_| ValidatorKeypair::generate()).collect();

    // Save individual key files
    for (i, kp) in keypairs.iter().enumerate() {
        let key_path = output_dir.join(format!("validator-{}.key", i + 1));
        kp.save_to_file(&key_path)
            .with_context(|| format!("failed to save key: {}", key_path.display()))?;
        println!(
            "  validator-{}: address={:?}, key={}",
            i + 1,
            kp.address(),
            key_path.display()
        );
    }

    // Build genesis config
    let genesis = GenesisConfig::from_keypairs(chain_config, &keypairs, stake_per_validator);
    genesis.validate().context("genesis config validation failed")?;

    let genesis_path = output_dir.join("genesis.json");
    let genesis_json = serde_json::to_string_pretty(&genesis)
        .context("failed to serialize genesis config")?;
    std::fs::write(&genesis_path, &genesis_json)
        .with_context(|| format!("failed to write {}", genesis_path.display()))?;

    println!("\nGenesis written to: {}", genesis_path.display());
    println!(
        "  validators: {}, stake/each: {}, total_stake: {}",
        num_validators,
        stake_per_validator,
        stake_per_validator * num_validators as u128
    );
    println!("  chain: {} ({})", genesis.chain_config.chain.chain_id, network);

    // Print env var hints for docker-compose
    println!("\nDocker environment variables:");
    println!("  AETHER_GENESIS_PATH={}", genesis_path.display());
    for (i, _) in keypairs.iter().enumerate() {
        let key_path = output_dir.join(format!("validator-{}.key", i + 1));
        println!("  AETHER_VALIDATOR_KEY={} (validator-{})", key_path.display(), i + 1);
    }

    Ok(())
}
