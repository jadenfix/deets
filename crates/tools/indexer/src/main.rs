use aether_rpc_grpc::FirehoseServer;
use aether_types::{Block, H256};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// In-memory indexed block store.
/// Production would use Postgres; this provides the structural wiring.
#[derive(Debug, Default)]
struct IndexerStore {
    blocks: HashMap<u64, IndexedBlock>,
    tx_to_block: HashMap<H256, u64>,
    latest_slot: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexedBlock {
    slot: u64,
    hash: H256,
    tx_count: usize,
    proposer: String,
}

impl IndexerStore {
    fn ingest(&mut self, block: &Block) {
        let indexed = IndexedBlock {
            slot: block.header.slot,
            hash: block.hash(),
            tx_count: block.transactions.len(),
            proposer: format!("{:?}", block.header.proposer),
        };

        for tx in &block.transactions {
            self.tx_to_block.insert(tx.hash(), block.header.slot);
        }

        if block.header.slot > self.latest_slot {
            self.latest_slot = block.header.slot;
        }

        self.blocks.insert(block.header.slot, indexed);
    }

    fn get_block(&self, slot: u64) -> Option<&IndexedBlock> {
        self.blocks.get(&slot)
    }

    fn block_count(&self) -> usize {
        self.blocks.len()
    }
}

/// Minimal HTTP query API for the indexer.
async fn run_query_api(store: Arc<RwLock<IndexerStore>>, port: u16) -> Result<()> {
    use warp::Filter;

    let store_filter = {
        let store = store.clone();
        warp::any().map(move || store.clone())
    };

    let status = warp::get()
        .and(warp::path("status"))
        .and(store_filter.clone())
        .map(|store: Arc<RwLock<IndexerStore>>| {
            let s = store.read().unwrap();
            warp::reply::json(&serde_json::json!({
                "blocks_indexed": s.block_count(),
                "latest_slot": s.latest_slot,
            }))
        });

    let block = warp::get()
        .and(warp::path("block"))
        .and(warp::path::param::<u64>())
        .and(store_filter.clone())
        .map(|slot: u64, store: Arc<RwLock<IndexerStore>>| {
            let s = store.read().unwrap();
            match s.get_block(slot) {
                Some(b) => warp::reply::json(&serde_json::to_value(b).unwrap()),
                None => warp::reply::json(&serde_json::json!(null)),
            }
        });

    let routes = status.or(block);
    println!("Indexer query API on http://127.0.0.1:{port}");
    warp::serve(routes).run(([127, 0, 0, 1], port)).await;
    Ok(())
}

/// Firehose ingestion loop.
async fn run_ingestion(firehose: &FirehoseServer, store: Arc<RwLock<IndexerStore>>) -> Result<()> {
    let mut stream = firehose.subscribe();
    println!("Indexer ingestion started, waiting for blocks...");

    loop {
        match stream.next().await {
            Some(event) => {
                let slot = event.block.header.slot;
                let tx_count = event.block.transactions.len();
                {
                    let mut s = store.write().unwrap();
                    s.ingest(&event.block);
                }
                println!("Indexed block slot={slot} txs={tx_count}");
            }
            None => {
                println!("Firehose stream ended");
                break;
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Aether Indexer v0.1.0");
    println!("====================\n");

    let query_port: u16 = std::env::var("INDEXER_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8081);

    let store = Arc::new(RwLock::new(IndexerStore::default()));

    // In production, the firehose would connect to a running node via gRPC.
    // For now, we create a local server for structural wiring.
    let firehose = FirehoseServer::new(256);

    let store_clone = store.clone();
    let ingestion = tokio::spawn(async move { run_ingestion(&firehose, store_clone).await });

    let api = tokio::spawn(run_query_api(store, query_port));

    tokio::select! {
        res = ingestion => { res??; }
        res = api => { res??; }
    }

    Ok(())
}
