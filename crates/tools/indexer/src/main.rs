use aether_indexer::PersistentStore;
use aether_types::Block;
use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Clone)]
struct IndexerConfig {
    rpc_url: String,
    bind_addr: IpAddr,
    port: u16,
    db_path: PathBuf,
    poll_interval: Duration,
}

impl IndexerConfig {
    fn from_env() -> Result<Self> {
        let rpc_url = std::env::var("INDEXER_RPC_URL")
            .or_else(|_| std::env::var("RPC_URL"))
            .unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());

        let bind_addr = std::env::var("INDEXER_BIND")
            .ok()
            .map(|value| {
                value
                    .parse::<IpAddr>()
                    .with_context(|| format!("invalid INDEXER_BIND '{}'", value))
            })
            .transpose()?
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));

        let port = match std::env::var("INDEXER_PORT") {
            Ok(value) => value
                .parse::<u16>()
                .with_context(|| format!("invalid INDEXER_PORT '{}'", value))?,
            Err(_) => 8081,
        };

        let db_path = std::env::var("INDEXER_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data/indexer"));

        let poll_interval = match std::env::var("INDEXER_POLL_INTERVAL_MS") {
            Ok(value) => Duration::from_millis(
                value
                    .parse::<u64>()
                    .with_context(|| format!("invalid INDEXER_POLL_INTERVAL_MS '{}'", value))?,
            ),
            Err(_) => Duration::from_millis(1_000),
        };

        Ok(Self {
            rpc_url,
            bind_addr,
            port,
            db_path,
            poll_interval,
        })
    }
}

async fn run_query_api(store: Arc<PersistentStore>, bind_addr: IpAddr, port: u16) -> Result<()> {
    use warp::Filter;

    let store_filter = {
        let store = store.clone();
        warp::any().map(move || store.clone())
    };

    let status = warp::get()
        .and(warp::path("status"))
        .and(store_filter.clone())
        .and_then(|store: Arc<PersistentStore>| async move {
            let blocks_indexed = store.block_count().unwrap_or(0);
            let latest_slot = store.latest_slot().unwrap_or(0);
            Ok::<_, std::convert::Infallible>(warp::reply::json(&json!({
                "blocks_indexed": blocks_indexed,
                "latest_slot": latest_slot,
            })))
        });

    let block = warp::get()
        .and(warp::path("block"))
        .and(warp::path::param::<u64>())
        .and(store_filter)
        .and_then(|slot: u64, store: Arc<PersistentStore>| async move {
            let reply = match store.get_block(slot) {
                Ok(Some(block)) => warp::reply::json(&json!(block)),
                Ok(None) => warp::reply::json(&json!(null)),
                Err(err) => warp::reply::json(&json!({ "error": err.to_string() })),
            };
            Ok::<_, std::convert::Infallible>(reply)
        });

    let routes = status.or(block);
    println!("Indexer query API on http://{}:{port}", bind_addr);
    warp::serve(routes).run((bind_addr, port)).await;
    Ok(())
}

async fn fetch_block(client: &Client, rpc_url: &str, block_ref: &str) -> Result<Option<Block>> {
    let response = client
        .post(rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "aeth_getBlockByNumber",
            "params": [block_ref, false],
            "id": 1,
        }))
        .send()
        .await
        .with_context(|| format!("failed to reach RPC at {}", rpc_url))?
        .error_for_status()
        .with_context(|| format!("RPC returned HTTP error for {}", rpc_url))?;

    let payload: JsonRpcResponse<Option<Block>> = response
        .json()
        .await
        .context("failed to decode RPC block response")?;

    if let Some(error) = payload.error {
        bail!(
            "RPC error while fetching block '{}': {}",
            block_ref,
            error.message
        );
    }

    Ok(payload.result.flatten())
}

async fn run_ingestion(config: &IndexerConfig, store: Arc<PersistentStore>) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("failed to build HTTP client")?;

    let mut next_slot = store.latest_slot().unwrap_or(0);
    if next_slot > 0 {
        next_slot += 1;
    }

    println!("Indexer ingestion polling {}", config.rpc_url);

    loop {
        match fetch_block(&client, &config.rpc_url, "latest").await {
            Ok(Some(latest_block)) => {
                let latest_slot = latest_block.header.slot;
                while next_slot <= latest_slot {
                    match fetch_block(&client, &config.rpc_url, &next_slot.to_string()).await {
                        Ok(Some(block)) => {
                            store.ingest(&block)?;
                            println!(
                                "Indexed block slot={} txs={}",
                                block.header.slot,
                                block.transactions.len()
                            );
                        }
                        Ok(None) => {
                            println!("No block at slot {}, skipping", next_slot);
                        }
                        Err(err) => {
                            eprintln!("WARNING: failed to fetch block {}: {}", next_slot, err);
                            break;
                        }
                    }
                    next_slot += 1;
                }
            }
            Ok(None) => {}
            Err(err) => {
                eprintln!("WARNING: failed to fetch latest block: {}", err);
            }
        }

        tokio::time::sleep(config.poll_interval).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Aether Indexer v0.1.0");
    println!("====================\n");

    let config = IndexerConfig::from_env()?;
    let store = Arc::new(PersistentStore::open(&config.db_path)?);

    let store_for_ingestion = store.clone();
    let config_for_ingestion = config.clone();
    let ingestion =
        tokio::spawn(
            async move { run_ingestion(&config_for_ingestion, store_for_ingestion).await },
        );

    let api = tokio::spawn(run_query_api(store, config.bind_addr, config.port));

    tokio::select! {
        res = ingestion => { res??; }
        res = api => { res??; }
    }

    Ok(())
}
