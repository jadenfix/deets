use anyhow::Result;
use clap::{Args, Subcommand};

use crate::config::ResolvedConfig;
use crate::io::parse_address;
use crate::transfers::{perform_transfer, TransferParams};

const STAKING_DELEGATE_ADDRESS: &str = "0x00000000000000000000000000000000000000ab";
const STAKING_WITHDRAW_ADDRESS: &str = "0x00000000000000000000000000000000000000ac";

#[derive(Subcommand, Debug)]
pub enum StakeCommand {
    /// Delegate SWR to the staking program
    Delegate(StakeArgs),
    /// Withdraw SWR from the staking program
    Withdraw(StakeArgs),
}

impl StakeCommand {
    pub async fn execute(&self, config: &ResolvedConfig) -> Result<()> {
        let (target, args) = match self {
            StakeCommand::Delegate(args) => (STAKING_DELEGATE_ADDRESS, args),
            StakeCommand::Withdraw(args) => (STAKING_WITHDRAW_ADDRESS, args),
        };

        let recipient = parse_address(target)?;
        let memo = args.memo.clone().unwrap_or_else(|| match self {
            StakeCommand::Delegate(_) => "stake:delegate".to_string(),
            StakeCommand::Withdraw(_) => "stake:withdraw".to_string(),
        });

        let params = TransferParams {
            recipient,
            amount: args.amount,
            nonce: args.nonce,
            memo: Some(memo),
            fee: args.fee,
            gas_limit: args.gas_limit,
        };
        let summary = perform_transfer(config, args.key.as_deref(), params).await?;

        println!("{}", serde_json::to_string_pretty(&summary)?);
        Ok(())
    }
}

#[derive(Args, Debug)]
pub struct StakeArgs {
    /// Amount of SWR tokens to move
    #[arg(long)]
    pub amount: u128,

    /// Sender nonce
    #[arg(long)]
    pub nonce: u64,

    /// Optional memo override
    #[arg(long)]
    pub memo: Option<String>,

    /// Override default fee
    #[arg(long)]
    pub fee: Option<u128>,

    /// Override default gas limit
    #[arg(long)]
    pub gas_limit: Option<u64>,

    /// Signing key path
    #[arg(long)]
    pub key: Option<String>,
}
