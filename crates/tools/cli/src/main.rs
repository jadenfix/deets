mod config;
mod genesis;
mod io;
mod jobs;
mod keys;
mod peers;
mod run;
mod snapshots;
mod staking;
mod status;
mod transfers;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::load_config;
use crate::genesis::InitGenesisCommand;
use crate::jobs::JobCommands;
use crate::keys::KeyCommands;
use crate::peers::PeersCommand;
use crate::run::RunCommand;
use crate::snapshots::SnapshotsCommand;
use crate::staking::StakeCommand;
use crate::status::StatusCommand;
use crate::transfers::TransferCommand;

#[derive(Parser, Debug)]
#[command(name = "aetherctl")]
#[command(version)]
#[command(about = "Command-line interface for the Aether blockchain")]
struct Cli {
    /// Override configuration file path (defaults to ~/.aether/config.toml)
    #[arg(long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize genesis configuration
    InitGenesis(InitGenesisCommand),
    /// Run an Aether node
    Run(RunCommand),
    /// Inspect chain configuration and local defaults
    Status(StatusCommand),
    /// Query connected peers
    Peers(PeersCommand),
    /// Manage state snapshots
    Snapshots(SnapshotsCommand),
    /// Generate or inspect key material
    Keys {
        #[command(subcommand)]
        command: KeyCommands,
    },
    /// Transfer tokens between accounts
    Transfer(TransferCommand),
    /// Stake helper commands (delegation via staking contract)
    Stake {
        #[command(subcommand)]
        command: StakeCommand,
    },
    /// Manage AI job submissions
    Job {
        #[command(subcommand)]
        command: JobCommands,
    },
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error: {err:?}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let config_path = cli.config.as_deref();
    let resolved = load_config(config_path)?;

    match cli.command {
        Commands::InitGenesis(cmd) => cmd.execute(&resolved).await?,
        Commands::Run(cmd) => cmd.execute(&resolved).await?,
        Commands::Status(cmd) => cmd.execute(&resolved).await?,
        Commands::Peers(cmd) => cmd.execute(&resolved).await?,
        Commands::Snapshots(cmd) => cmd.execute(&resolved).await?,
        Commands::Keys { command } => command.execute(&resolved).await?,
        Commands::Transfer(cmd) => cmd.execute(&resolved).await?,
        Commands::Stake { command } => command.execute(&resolved).await?,
        Commands::Job { command } => command.execute(&resolved).await?,
    }

    Ok(())
}
