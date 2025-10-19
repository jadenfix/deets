use crate::config::ResolvedConfig;
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use reqwest::Client;
use serde_json::{json, Value};

#[derive(Parser, Debug)]
pub struct PeersCommand {
    /// JSON-RPC endpoint to query
    #[arg(long, default_value = "http://127.0.0.1:8545")]
    rpc: String,

    /// Request detailed peer information when available
    #[arg(long)]
    verbose: bool,
}

impl PeersCommand {
    pub async fn execute(&self, _config: &ResolvedConfig) -> Result<()> {
        let client = Client::new();

        let peer_count = self
            .rpc_call(&client, "net_peerCount", Value::Array(vec![]))
            .await
            .context("failed to query peer count")?;

        match peer_count {
            Value::String(ref s) => println!("Connected peers: {}", s),
            Value::Number(ref n) => println!("Connected peers: {}", n),
            other => println!("Connected peers: {:?}", other),
        }

        if self.verbose {
            match self
                .rpc_call(&client, "aeth_getPeers", Value::Array(vec![]))
                .await
            {
                Ok(Value::Array(peers)) if !peers.is_empty() => {
                    println!("\nPeer details:");
                    for peer in peers {
                        println!("  {}", peer);
                    }
                }
                Ok(_) => println!("\nPeer details not available (empty response)"),
                Err(err) => {
                    println!("\nDetailed peer information unavailable: {}", err);
                }
            }
        }

        Ok(())
    }

    async fn rpc_call(&self, client: &Client, method: &str, params: Value) -> Result<Value> {
        let request = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        });

        let response = client
            .post(&self.rpc)
            .json(&request)
            .send()
            .await
            .with_context(|| format!("failed to reach RPC endpoint {}", self.rpc))?;

        let body: Value = response.json().await.context("invalid JSON-RPC response")?;

        if let Some(error) = body.get("error") {
            return Err(anyhow!(error.to_string()));
        }

        body.get("result")
            .cloned()
            .ok_or_else(|| anyhow!("missing result field in RPC response"))
    }
}
