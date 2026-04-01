use std::env;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use aether_node::{
    create_hybrid_consensus, create_hybrid_consensus_with_all_keys, validator_info_from_keypair,
    GenesisConfig, Node, OutboundMessage, ValidatorKeypair,
};
use aether_p2p::network::{P2PNetwork, TOPIC_VOTE};
use aether_rpc_json::{JsonRpcServer, RpcBackend};
use aether_types::{Address, Block, ChainConfig, Transaction, TransactionReceipt, H256};
use anyhow::{Context, Result};
use serde_json::Value;
use tokio::sync::mpsc;

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

    fn get_latest_block_slot(&self) -> Result<Option<u64>> {
        let node = self.read_node()?;
        Ok(node.latest_block_slot())
    }

    fn request_airdrop(&self, address: Address, amount: u128) -> Result<()> {
        let mut node = self.write_node()?;
        node.seed_account(&address, amount)
    }
}

async fn run_slot_loop(
    node: Arc<RwLock<Node>>,
    mut net_rx: mpsc::UnboundedReceiver<aether_p2p::network::NetworkEvent>,
    slot_ms: u64,
) -> Result<()> {
    loop {
        // Drain all pending network messages
        while let Ok(event) = net_rx.try_recv() {
            let mut guard = node
                .write()
                .map_err(|_| anyhow::anyhow!("node lock poisoned"))?;
            guard.handle_network_event(event)?;
        }
        // Tick the node
        {
            let mut guard = node
                .write()
                .map_err(|_| anyhow::anyhow!("node lock poisoned"))?;
            guard.tick()?;
        }
        tokio::time::sleep(Duration::from_millis(slot_ms)).await;
    }
}

/// P2P outbound loop: reads OutboundMessages from node and publishes to network.
async fn run_p2p_outbound(
    mut p2p: P2PNetwork,
    mut outbound_rx: mpsc::UnboundedReceiver<OutboundMessage>,
    net_tx: mpsc::UnboundedSender<aether_p2p::network::NetworkEvent>,
) -> Result<()> {
    loop {
        tokio::select! {
            // Poll for inbound P2P events
            event = p2p.poll() => {
                if let Some(event) = event {
                    let _ = net_tx.send(event);
                }
            }
            // Handle outbound messages from the node
            msg = outbound_rx.recv() => {
                match msg {
                    Some(OutboundMessage::BroadcastBlock(block)) => {
                        if let Err(e) = p2p.broadcast_block(&block) {
                            tracing::warn!("failed to broadcast block: {e}");
                        }
                    }
                    Some(OutboundMessage::BroadcastVote(vote)) => {
                        let data = bincode::serialize(&vote).unwrap_or_default();
                        if let Err(e) = p2p.publish(TOPIC_VOTE, data) {
                            tracing::warn!("failed to broadcast vote: {e}");
                        }
                    }
                    Some(OutboundMessage::BroadcastTransaction(tx)) => {
                        if let Err(e) = p2p.broadcast_transaction(&tx) {
                            tracing::warn!("failed to broadcast tx: {e}");
                        }
                    }
                    None => break, // Channel closed
                }
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Aether Node v0.3.0");
    println!("=================\n");

    // Load chain configuration
    let chain_config = if let Ok(config_path) = env::var("AETHER_CONFIG_PATH") {
        println!("Loading config from: {config_path}");
        ChainConfig::from_toml_file(Path::new(&config_path))?
    } else {
        let network = env::var("AETHER_NETWORK").unwrap_or_else(|_| "devnet".to_string());
        println!("Using {network} preset config");
        match network.as_str() {
            "mainnet" => ChainConfig::mainnet(),
            "testnet" => ChainConfig::testnet(),
            _ => ChainConfig::devnet(),
        }
    };

    let chain_config = Arc::new(chain_config);
    println!(
        "Chain: {} (numeric ID: {})",
        chain_config.chain.chain_id, chain_config.chain.chain_id_numeric
    );

    let db_path = env::var("AETHER_NODE_DB_PATH").unwrap_or_else(|_| "./data/node1".to_string());

    // Load or generate validator keypair
    let key_path =
        env::var("AETHER_VALIDATOR_KEY").unwrap_or_else(|_| format!("{}/validator.key", db_path));
    let key_path = std::path::Path::new(&key_path);

    let validator_keypair = if key_path.exists() {
        println!("Loading validator key from: {}", key_path.display());
        ValidatorKeypair::load_from_file(key_path)?
    } else {
        println!("Generating new validator keypair...");
        let kp = ValidatorKeypair::generate();
        // Auto-save so the node keeps the same identity on restart
        if let Err(e) = kp.save_to_file(key_path) {
            eprintln!("WARNING: failed to save validator key: {e}");
        } else {
            println!("Saved validator key to: {}", key_path.display());
        }
        kp
    };
    let validator_address = validator_keypair.address();

    // Build consensus from genesis file (multi-validator) or single-validator mode
    let consensus: Box<dyn aether_consensus::ConsensusEngine> =
        if let Ok(genesis_path) = env::var("AETHER_GENESIS_PATH") {
            println!("Loading genesis from: {genesis_path}");
            let genesis_bytes = std::fs::read(&genesis_path)
                .with_context(|| format!("failed to read genesis file: {genesis_path}"))?;
            let genesis: GenesisConfig = serde_json::from_slice(&genesis_bytes)
                .with_context(|| "failed to parse genesis JSON")?;
            genesis.validate()?;

            let result = genesis.build();
            let vrf_pubkeys = genesis.vrf_pubkeys();
            let bls_pubkeys = genesis.bls_pubkeys();

            println!(
                "Genesis: {} validators, {} total stake",
                result.validator_set.len(),
                result.total_stake
            );

            Box::new(create_hybrid_consensus_with_all_keys(
                result.validator_set,
                vrf_pubkeys,
                bls_pubkeys,
                Some(&validator_keypair),
                chain_config.consensus.tau,
                chain_config.chain.epoch_slots,
            )?)
        } else {
            // Single-validator quick-start mode
            let validators = vec![validator_info_from_keypair(&validator_keypair, 1_000_000)];
            Box::new(create_hybrid_consensus(
                validators,
                Some(&validator_keypair),
                chain_config.consensus.tau,
                chain_config.chain.epoch_slots,
            )?)
        };

    let rpc_port: u16 = env::var("AETHER_RPC_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8545);
    let p2p_port: u16 = env::var("AETHER_P2P_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9000);

    let mut node = Node::new(
        db_path,
        consensus,
        Some(validator_keypair.ed25519),
        Some(validator_keypair.bls),
        chain_config.clone(),
    )?;

    // Seed validator with genesis balance (only on first run)
    let genesis_balance = chain_config.tokens.swr_initial_supply;
    if node.get_account(validator_address)?.is_none() {
        node.seed_account(&validator_address, genesis_balance)?;
        println!("Genesis: funded validator with {genesis_balance}");
    } else {
        println!("Node already initialized, skipping genesis funding");
    }

    // Set up P2P outbound channel
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
    node.set_broadcast_tx(outbound_tx);

    // Set up P2P inbound channel
    let (net_tx, net_rx) = mpsc::unbounded_channel();

    let shared_node = Arc::new(RwLock::new(node));

    let backend = NodeRpcBackend {
        node: shared_node.clone(),
    };
    let rpc_server = JsonRpcServer::new(backend, rpc_port);

    // Initialize P2P network
    let mut p2p = P2PNetwork::new_random()?;
    let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", p2p_port);
    p2p.start(&listen_addr).await?;
    let peer_id = p2p.peer_id_str();

    println!("Validator address: {:?}", validator_address);
    println!("Peer ID: {}", peer_id);
    println!("Consensus: VRF + HotStuff + BLS");
    println!("P2P listening on 0.0.0.0:{p2p_port}");
    println!("JSON-RPC listening on 127.0.0.1:{rpc_port}");
    println!(
        "Slot duration: {}ms, Epoch: {} slots",
        chain_config.chain.slot_ms, chain_config.chain.epoch_slots
    );
    println!("Press Ctrl-C to stop.\n");

    // Connect to bootstrap peers if specified
    if let Ok(peers) = env::var("AETHER_BOOTSTRAP_PEERS") {
        for peer_addr in peers.split(',') {
            let addr = peer_addr.trim();
            if !addr.is_empty() {
                match p2p.connect_peer(addr) {
                    Ok(()) => println!("Connecting to peer: {addr}"),
                    Err(e) => println!("Failed to connect to {addr}: {e}"),
                }
            }
        }
    }

    let slot_ms = chain_config.chain.slot_ms;
    let slot_task = tokio::spawn(run_slot_loop(shared_node, net_rx, slot_ms));
    let rpc_task = tokio::spawn(async move { rpc_server.run().await });
    let p2p_task = tokio::spawn(run_p2p_outbound(p2p, outbound_rx, net_tx));

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
        res = p2p_task => {
            match res {
                Ok(inner) => inner?,
                Err(e) => return Err(anyhow::anyhow!("p2p task failed: {e}")),
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nReceived Ctrl-C, shutting down...");
        }
    }

    Ok(())
}
