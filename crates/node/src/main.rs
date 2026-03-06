use std::env;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use aether_node::{create_hybrid_consensus, validator_info_from_keypair, Node, ValidatorKeypair};
use aether_rpc_json::{JsonRpcServer, RpcBackend};
use aether_types::{Address, Block, Transaction, TransactionReceipt, H256};
use anyhow::{Context, Result};
use serde_json::Value;

struct NodeRpcBackend {
    node: Arc<RwLock<Node>>,
}

impl NodeRpcBackend {
    fn read_node(&self) -> Result<std::sync::RwLockReadGuard<'_, Node>> {
        self.node
            .read()
            .map_err(|_| anyhow::anyhow!("node lock poisoned"))
    }

    fn write_node(&self) -> Result<std::sync::RwLockWriteGuard<'_, Node>> {
        self.node
            .write()
            .map_err(|_| anyhow::anyhow!("node lock poisoned"))
    }
}

impl RpcBackend for NodeRpcBackend {
    fn send_raw_transaction(&self, tx_bytes: Vec<u8>) -> Result<H256> {
        let tx: Transaction =
            bincode::deserialize(&tx_bytes).context("failed to decode transaction bytes")?;
        let mut node = self.write_node()?;
        node.submit_transaction(tx)
    }

    fn get_block_by_number(&self, block_number: u64, _full_tx: bool) -> Result<Option<Block>> {
        let node = self.read_node()?;
        Ok(node.get_block_by_slot(block_number))
    }

    fn get_block_by_hash(&self, block_hash: H256, _full_tx: bool) -> Result<Option<Block>> {
        let node = self.read_node()?;
        Ok(node.get_block_by_hash(block_hash))
    }

    fn get_transaction_receipt(&self, tx_hash: H256) -> Result<Option<TransactionReceipt>> {
        let node = self.read_node()?;
        Ok(node.get_transaction_receipt(tx_hash))
    }

    fn get_state_root(&self, _block_ref: Option<String>) -> Result<H256> {
        let node = self.read_node()?;
        Ok(node.get_state_root())
    }

    fn get_account(&self, address: Address, _block_ref: Option<String>) -> Result<Option<Value>> {
        let node = self.read_node()?;
        match node.get_account(address)? {
            Some(account) => Ok(Some(serde_json::to_value(account)?)),
            None => Ok(None),
        }
    }

    fn get_slot_number(&self) -> Result<u64> {
        let node = self.read_node()?;
        Ok(node.current_slot())
    }

    fn get_finalized_slot(&self) -> Result<u64> {
        let node = self.read_node()?;
        Ok(node.finalized_slot())
    }
}

async fn run_slot_loop(node: Arc<RwLock<Node>>) -> Result<()> {
    loop {
        {
            let mut guard = node
                .write()
                .map_err(|_| anyhow::anyhow!("node lock poisoned"))?;
            guard.tick()?;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Aether Node v0.1.0");
    println!("=================\n");

    let validator_keypair = ValidatorKeypair::generate();
    let validators = vec![validator_info_from_keypair(&validator_keypair, 1_000_000)];
    let validator_address = validator_keypair.address();

    let consensus = Box::new(create_hybrid_consensus(
        validators,
        Some(&validator_keypair),
        0.8, // tau: leader rate
        100, // epoch length in slots
    )?);

    let db_path = env::var("AETHER_NODE_DB_PATH").unwrap_or_else(|_| "./data/node1".to_string());
    let rpc_port: u16 = env::var("AETHER_RPC_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8545);

    let node = Node::new(db_path, consensus, Some(validator_keypair.ed25519))?;
    let shared_node = Arc::new(RwLock::new(node));

    let backend = NodeRpcBackend {
        node: shared_node.clone(),
    };
    let rpc_server = JsonRpcServer::new(backend, rpc_port);

    println!("Validator address: {:?}", validator_address);
    println!("Consensus: VRF + HotStuff + BLS");
    println!("JSON-RPC listening on 127.0.0.1:{rpc_port}");
    println!("Press Ctrl-C to stop.\n");

    let slot_task = tokio::spawn(run_slot_loop(shared_node));
    let rpc_task = tokio::spawn(async move { rpc_server.run().await });

    tokio::select! {
        res = slot_task => {
            match res {
                Ok(inner) => inner?,
                Err(e) => return Err(anyhow::anyhow!("slot loop task failed: {e}")),
            }
        }
        res = rpc_task => {
            match res {
                Ok(inner) => inner?,
                Err(e) => return Err(anyhow::anyhow!("rpc task failed: {e}")),
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nReceived Ctrl-C, shutting down...");
        }
    }

    Ok(())
}
