use aether_types::{
    Address, Block, PublicKey, Signature, Transaction, TransactionReceipt, TransferPayload, H256,
    TRANSFER_PROGRAM_ID,
};
use anyhow::Result;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use warp::ws::{Message, WebSocket};
use warp::{Filter, Reply};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Vec<Value>,
    pub id: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcTransferRequest {
    nonce: u64,
    sender: String,
    #[serde(alias = "senderPublicKey")]
    sender_public_key: String,
    recipient: String,
    amount: Value,
    fee: Value,
    #[serde(alias = "gasLimit")]
    gas_limit: u64,
    memo: Option<String>,
    #[serde(default)]
    reads: Vec<String>,
    #[serde(default)]
    writes: Vec<String>,
    signature: String,
}

pub trait RpcBackend: Send + Sync {
    fn send_raw_transaction(&self, tx_bytes: Vec<u8>) -> Result<H256>;
    fn get_block_by_number(&self, block_number: u64, full_tx: bool) -> Result<Option<Block>>;
    fn get_block_by_hash(&self, block_hash: H256, full_tx: bool) -> Result<Option<Block>>;
    fn get_transaction_receipt(&self, tx_hash: H256) -> Result<Option<TransactionReceipt>>;
    fn get_state_root(&self, block_ref: Option<String>) -> Result<H256>;
    fn get_account(&self, address: Address, block_ref: Option<String>) -> Result<Option<Value>>;
    fn get_slot_number(&self) -> Result<u64>;
    fn get_finalized_slot(&self) -> Result<u64>;
    fn get_latest_block_slot(&self) -> Result<Option<u64>> {
        Ok(None)
    }
    fn get_peer_count(&self) -> Result<usize> {
        Ok(0)
    }
    fn get_sync_status(&self) -> Result<Value> {
        Ok(json!({"syncing": false}))
    }
    fn allows_airdrop(&self) -> bool {
        false
    }
    fn request_airdrop(&self, _address: Address, _amount: u128) -> Result<()> {
        Err(anyhow::anyhow!("airdrop not supported"))
    }
}

/// Subscription topics for WebSocket clients.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubscriptionTopic {
    NewBlocks,
    NewTransactions,
    Finality,
}

/// Event broadcast to WebSocket subscribers.
#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionEvent {
    pub topic: String,
    pub data: Value,
}

/// Manages WebSocket subscriptions and event broadcasting.
pub struct SubscriptionManager {
    sender: broadcast::Sender<SubscriptionEvent>,
}

impl SubscriptionManager {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1024);
        SubscriptionManager { sender }
    }

    /// Broadcast a new block event to all subscribers.
    pub fn notify_new_block(&self, block: &Block) {
        let event = SubscriptionEvent {
            topic: "newBlock".to_string(),
            data: json!({
                "slot": block.header.slot,
                "hash": format!("0x{}", hex::encode(block.hash().as_bytes())),
                "proposer": format!("0x{}", hex::encode(block.header.proposer.as_bytes())),
                "txCount": block.transactions.len(),
                "timestamp": block.header.timestamp,
            }),
        };
        if let Err(e) = self.sender.send(event) {
            tracing::debug!("No active subscribers for new block event: {e}");
        }
    }

    /// Broadcast a finality event.
    pub fn notify_finality(&self, slot: u64, block_hash: H256) {
        let event = SubscriptionEvent {
            topic: "finality".to_string(),
            data: json!({
                "finalizedSlot": slot,
                "blockHash": format!("0x{}", hex::encode(block_hash.as_bytes())),
            }),
        };
        if let Err(e) = self.sender.send(event) {
            tracing::debug!("No active subscribers for finality event: {e}");
        }
    }

    /// Get a new subscriber receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<SubscriptionEvent> {
        self.sender.subscribe()
    }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct JsonRpcServer<B: RpcBackend> {
    backend: Arc<RwLock<B>>,
    subscriptions: Arc<SubscriptionManager>,
    port: u16,
    /// The chain ID of the network this server is serving.  Stamped onto
    /// every `aeth_sendTransaction` transaction so callers cannot forge
    /// cross-chain replays via this endpoint.
    chain_id: u64,
}

impl<B: RpcBackend + 'static> JsonRpcServer<B> {
    pub fn new(backend: B, port: u16) -> Self {
        Self {
            backend: Arc::new(RwLock::new(backend)),
            subscriptions: Arc::new(SubscriptionManager::new()),
            port,
            // Default to mainnet chain_id = 1; use `with_chain_id` to override.
            chain_id: 1,
        }
    }

    /// Construct a server for a specific chain (e.g. testnet, devnet).
    pub fn with_chain_id(backend: B, port: u16, chain_id: u64) -> Self {
        Self {
            backend: Arc::new(RwLock::new(backend)),
            subscriptions: Arc::new(SubscriptionManager::new()),
            port,
            chain_id,
        }
    }

    /// Get a reference to the subscription manager for event broadcasting.
    pub fn subscription_manager(&self) -> Arc<SubscriptionManager> {
        self.subscriptions.clone()
    }

    pub async fn run(self) -> Result<()> {
        let backend = self.backend.clone();
        let subs = self.subscriptions.clone();
        let chain_id = self.chain_id;

        let rpc = warp::post()
            .and(warp::path::end())
            .and(warp::body::content_length_limit(1024 * 256)) // 256KB max
            .and(warp::body::json())
            .and(with_backend(backend))
            .and(with_chain_id(chain_id))
            .and_then(handle_rpc_request);

        let health_backend = self.backend.clone();
        let health = warp::get()
            .and(warp::path("health"))
            .and_then(move || {
                let backend = health_backend.clone();
                async move {
                    let backend = backend.read().await;
                    let slot = backend.get_slot_number().unwrap_or(0);
                    let finalized = backend.get_finalized_slot().unwrap_or(0);
                    let peer_count = backend.get_peer_count().unwrap_or(0);
                    let sync_status = backend
                        .get_sync_status()
                        .unwrap_or_else(|_| json!({"syncing": false}));
                    Ok::<_, warp::Rejection>(warp::reply::json(&json!({
                        "status": "ok",
                        "version": env!("CARGO_PKG_VERSION"),
                        "latestSlot": slot,
                        "finalizedSlot": finalized,
                        "peerCount": peer_count,
                        "sync": sync_status,
                    })))
                }
            });

        // WebSocket subscription endpoint
        let ws_subs = subs.clone();
        let ws = warp::path("ws")
            .and(warp::ws())
            .map(move |ws: warp::ws::Ws| {
                let subs = ws_subs.clone();
                ws.on_upgrade(move |socket| handle_ws_connection(socket, subs))
            });

        let cors_origins: Vec<String> = std::env::var("CORS_ORIGINS")
            .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_else(|_| {
                vec![
                    "http://localhost:3000".to_string(),
                    "http://127.0.0.1:3000".to_string(),
                ]
            });
        let mut cors = warp::cors()
            .allow_methods(vec!["POST", "GET", "OPTIONS"])
            .allow_headers(vec!["content-type"]);
        for origin in &cors_origins {
            cors = cors.allow_origin(origin.as_str());
        }

        let routes = rpc.or(health).or(ws).with(cors);

        tracing::info!("JSON-RPC server listening on 127.0.0.1:{}", self.port);
        tracing::info!("WebSocket subscriptions on ws://127.0.0.1:{}/ws", self.port);
        warp::serve(routes).run(([127, 0, 0, 1], self.port)).await;

        Ok(())
    }
}

async fn handle_ws_connection(ws: WebSocket, subs: Arc<SubscriptionManager>) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    let mut rx = subs.subscribe();

    // Spawn task to forward subscription events to this WebSocket client
    let timeout_duration = Duration::from_secs(300); // 5 minute idle timeout
    let send_task = tokio::spawn(async move {
        loop {
            match tokio::time::timeout(timeout_duration, rx.recv()).await {
                Ok(Ok(event)) => {
                    let msg = match serde_json::to_string(&event) {
                        Ok(msg) => msg,
                        Err(e) => {
                            tracing::warn!("Failed to serialize subscription event: {e}");
                            continue;
                        }
                    };
                    if ws_tx.send(Message::text(msg)).await.is_err() {
                        break; // Client disconnected
                    }
                }
                Ok(Err(_)) => break, // Channel closed
                Err(_) => {
                    // Idle timeout reached, close connection
                    tracing::info!("WebSocket idle timeout reached, closing connection");
                    break;
                }
            }
        }
    });

    // Read client messages (subscribe/unsubscribe commands)
    while let Some(Ok(msg)) = ws_rx.next().await {
        if msg.is_close() {
            break;
        }
        // For now, just accept the connection. Future: parse subscribe/unsubscribe messages.
    }

    send_task.abort();
}

fn with_backend<B: RpcBackend>(
    backend: Arc<RwLock<B>>,
) -> impl Filter<Extract = (Arc<RwLock<B>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || backend.clone())
}

fn with_chain_id(
    chain_id: u64,
) -> impl Filter<Extract = (u64,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || chain_id)
}

async fn handle_rpc_request<B: RpcBackend>(
    req: JsonRpcRequest,
    backend: Arc<RwLock<B>>,
    chain_id: u64,
) -> Result<impl Reply, warp::Rejection> {
    let req_id = req.id.clone();
    let response = match tokio::time::timeout(
        Duration::from_secs(30),
        process_rpc_request(req, backend, chain_id),
    )
    .await
    {
        Ok(resp) => resp,
        Err(_) => {
            tracing::warn!("RPC request timed out after 30s");
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32000,
                    message: "Request timed out".to_string(),
                    data: None,
                }),
                id: req_id,
            }
        }
    };
    Ok(warp::reply::json(&response))
}

async fn process_rpc_request<B: RpcBackend>(
    req: JsonRpcRequest,
    backend: Arc<RwLock<B>>,
    chain_id: u64,
) -> JsonRpcResponse {
    let result = match req.method.as_str() {
        "aeth_sendRawTransaction" => handle_send_raw_transaction(&req.params, backend).await,
        "aeth_sendTransaction" => {
            handle_send_transaction(&req.params, backend, chain_id).await
        }
        "aeth_chainId" => Ok(json!(format!("0x{:x}", chain_id))),
        "aeth_getBlockByNumber" => handle_get_block_by_number(&req.params, backend).await,
        "aeth_getBlockByHash" => handle_get_block_by_hash(&req.params, backend).await,
        "aeth_getTransactionReceipt" => handle_get_transaction_receipt(&req.params, backend).await,
        "aeth_getStateRoot" => handle_get_state_root(&req.params, backend).await,
        "aeth_getAccount" => handle_get_account(&req.params, backend).await,
        "aeth_getSlotNumber" => handle_get_slot_number(backend).await,
        "aeth_getFinalizedSlot" => handle_get_finalized_slot(backend).await,
        "aeth_requestAirdrop" => handle_request_airdrop(&req.params, backend).await,
        "aeth_health" => handle_health(backend).await,
        _ => Err(JsonRpcError {
            code: -32601,
            message: format!("Method not found: {}", req.method),
            data: None,
        }),
    };

    match result {
        Ok(value) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(value),
            error: None,
            id: req.id,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(error),
            id: req.id,
        },
    }
}

async fn handle_send_raw_transaction<B: RpcBackend>(
    params: &[Value],
    backend: Arc<RwLock<B>>,
) -> Result<Value, JsonRpcError> {
    if params.is_empty() {
        return Err(JsonRpcError {
            code: -32602,
            message: "Missing parameter: tx_bytes".to_string(),
            data: None,
        });
    }

    let tx_hex = params[0].as_str().ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!(
            "Invalid parameter type: expected hex string, got {}",
            params[0]
        ),
        data: None,
    })?;

    let tx_bytes = hex::decode(tx_hex.trim_start_matches("0x")).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid hex: {}", e),
        data: None,
    })?;

    let backend = backend.read().await;
    let tx_hash = backend
        .send_raw_transaction(tx_bytes)
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Transaction rejected: {}", e),
            data: None,
        })?;

    Ok(json!(format!("0x{}", hex::encode(tx_hash.as_bytes()))))
}

async fn handle_send_transaction<B: RpcBackend>(
    params: &[Value],
    backend: Arc<RwLock<B>>,
    chain_id: u64,
) -> Result<Value, JsonRpcError> {
    if params.is_empty() {
        return Err(JsonRpcError {
            code: -32602,
            message: "Missing parameter: transaction".to_string(),
            data: None,
        });
    }

    let transfer: RpcTransferRequest =
        serde_json::from_value(params[0].clone()).map_err(|e| JsonRpcError {
            code: -32602,
            message: format!("Invalid transaction payload: {e}"),
            data: None,
        })?;

    let sender = parse_address(&transfer.sender, "sender")?;
    let sender_pubkey = parse_hex_bytes(&transfer.sender_public_key, "sender_public_key")?;
    let sender_pubkey = PublicKey::from_bytes(sender_pubkey);
    let signature = Signature::from_bytes(parse_hex_bytes(&transfer.signature, "signature")?);
    let recipient = parse_address(&transfer.recipient, "recipient")?;
    let amount = parse_u128_value(&transfer.amount, "amount")?;
    let fee = parse_u128_value(&transfer.fee, "fee")?;
    let mut reads = parse_address_set(&transfer.reads, "reads")?;
    let mut writes = parse_address_set(&transfer.writes, "writes")?;
    reads.insert(sender);
    writes.insert(sender);
    writes.insert(recipient);

    if sender_pubkey.to_address() != sender {
        return Err(JsonRpcError {
            code: -32602,
            message: "sender does not match sender_public_key".to_string(),
            data: None,
        });
    }

    let payload = TransferPayload {
        recipient,
        amount,
        memo: transfer.memo,
    };
    let data = bincode::serialize(&payload).map_err(|e| JsonRpcError {
        code: -32000,
        message: format!("Failed to encode transfer payload: {e}"),
        data: None,
    })?;

    let tx = Transaction {
        nonce: transfer.nonce,
        chain_id,
        sender,
        sender_pubkey,
        inputs: Vec::new(),
        outputs: Vec::new(),
        reads,
        writes,
        program_id: Some(TRANSFER_PROGRAM_ID),
        data,
        gas_limit: transfer.gas_limit,
        fee,
        signature,
    };

    let tx_bytes = bincode::serialize(&tx).map_err(|e| JsonRpcError {
        code: -32000,
        message: format!("Failed to encode transaction: {e}"),
        data: None,
    })?;

    let backend = backend.read().await;
    let tx_hash = backend
        .send_raw_transaction(tx_bytes)
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Transaction rejected: {e}"),
            data: None,
        })?;

    Ok(json!(format!("0x{}", hex::encode(tx_hash.as_bytes()))))
}

async fn handle_get_block_by_number<B: RpcBackend>(
    params: &[Value],
    backend: Arc<RwLock<B>>,
) -> Result<Value, JsonRpcError> {
    if params.len() < 2 {
        return Err(JsonRpcError {
            code: -32602,
            message: "Missing parameters".to_string(),
            data: None,
        });
    }

    let block_ref = params[0].as_str().ok_or_else(|| JsonRpcError {
        code: -32602,
        message: "Invalid block reference".to_string(),
        data: None,
    })?;

    let full_tx = params[1].as_bool().unwrap_or(false);

    let block_number = if block_ref == "latest" {
        let backend = backend.read().await;
        if let Some(slot) = backend.get_latest_block_slot().map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Failed to get latest block slot: {}", e),
            data: None,
        })? {
            slot
        } else {
            backend.get_slot_number().map_err(|e| JsonRpcError {
                code: -32000,
                message: format!("Failed to get latest slot: {}", e),
                data: None,
            })?
        }
    } else {
        block_ref.parse::<u64>().map_err(|_| JsonRpcError {
            code: -32602,
            message: "Invalid block number".to_string(),
            data: None,
        })?
    };

    let backend = backend.read().await;
    let block = backend
        .get_block_by_number(block_number, full_tx)
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Failed to get block: {}", e),
            data: None,
        })?;

    Ok(json!(block))
}

fn parse_address(value: &str, field: &str) -> Result<Address, JsonRpcError> {
    let bytes = parse_hex_bytes(value, field)?;
    Address::from_slice(&bytes).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid {field} length: {e}"),
        data: None,
    })
}

fn parse_address_set(values: &[String], field: &str) -> Result<HashSet<Address>, JsonRpcError> {
    let mut out = HashSet::new();
    for value in values {
        let addr = parse_address(value, field)?;
        out.insert(addr);
    }
    Ok(out)
}

fn parse_hex_bytes(value: &str, field: &str) -> Result<Vec<u8>, JsonRpcError> {
    hex::decode(value.trim_start_matches("0x")).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid {field} hex: {e}"),
        data: None,
    })
}

fn parse_u128_value(value: &Value, field: &str) -> Result<u128, JsonRpcError> {
    match value {
        Value::String(s) => s.parse::<u128>().map_err(|e| JsonRpcError {
            code: -32602,
            message: format!("Invalid {field}: {e}"),
            data: None,
        }),
        Value::Number(n) => n.as_u64().map(u128::from).ok_or_else(|| JsonRpcError {
            code: -32602,
            message: format!("Invalid {field}: expected unsigned integer"),
            data: None,
        }),
        _ => Err(JsonRpcError {
            code: -32602,
            message: format!("Invalid {field}: expected string or number"),
            data: None,
        }),
    }
}

async fn handle_get_block_by_hash<B: RpcBackend>(
    params: &[Value],
    backend: Arc<RwLock<B>>,
) -> Result<Value, JsonRpcError> {
    if params.len() < 2 {
        return Err(JsonRpcError {
            code: -32602,
            message: "Missing parameters".to_string(),
            data: None,
        });
    }

    let hash_hex = params[0].as_str().ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!(
            "Invalid hash: expected 0x-prefixed 64-char hex string, got {}",
            params[0]
        ),
        data: None,
    })?;

    let full_tx = params[1].as_bool().unwrap_or(false);

    let hash_bytes = hex::decode(hash_hex.trim_start_matches("0x")).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid hash hex '{}': {}", hash_hex, e),
        data: None,
    })?;

    let block_hash = H256::from_slice(&hash_bytes).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid hash length for '{}': {}", hash_hex, e),
        data: None,
    })?;

    let backend = backend.read().await;
    let block = backend
        .get_block_by_hash(block_hash, full_tx)
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Failed to get block: {}", e),
            data: None,
        })?;

    Ok(json!(block))
}

async fn handle_get_transaction_receipt<B: RpcBackend>(
    params: &[Value],
    backend: Arc<RwLock<B>>,
) -> Result<Value, JsonRpcError> {
    if params.is_empty() {
        return Err(JsonRpcError {
            code: -32602,
            message: "Missing parameter: tx_hash".to_string(),
            data: None,
        });
    }

    let hash_hex = params[0].as_str().ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!(
            "Invalid hash: expected 0x-prefixed 64-char hex string, got {}",
            params[0]
        ),
        data: None,
    })?;

    let hash_bytes = hex::decode(hash_hex.trim_start_matches("0x")).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid hash hex '{}': {}", hash_hex, e),
        data: None,
    })?;

    let tx_hash = H256::from_slice(&hash_bytes).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid hash length for '{}': {}", hash_hex, e),
        data: None,
    })?;

    let backend = backend.read().await;
    let receipt = backend
        .get_transaction_receipt(tx_hash)
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Failed to get receipt: {}", e),
            data: None,
        })?;

    Ok(json!(receipt))
}

async fn handle_get_state_root<B: RpcBackend>(
    params: &[Value],
    backend: Arc<RwLock<B>>,
) -> Result<Value, JsonRpcError> {
    let block_ref = params.first().and_then(|v| v.as_str()).map(String::from);

    let backend = backend.read().await;
    let state_root = backend
        .get_state_root(block_ref)
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Failed to get state root: {}", e),
            data: None,
        })?;

    Ok(json!(format!("0x{}", hex::encode(state_root.as_bytes()))))
}

async fn handle_get_account<B: RpcBackend>(
    params: &[Value],
    backend: Arc<RwLock<B>>,
) -> Result<Value, JsonRpcError> {
    if params.is_empty() {
        return Err(JsonRpcError {
            code: -32602,
            message: "Missing parameter: address".to_string(),
            data: None,
        });
    }

    let addr_hex = params[0].as_str().ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!(
            "Invalid address: expected 0x-prefixed 40-char hex string, got {}",
            params[0]
        ),
        data: None,
    })?;

    let addr_bytes = hex::decode(addr_hex.trim_start_matches("0x")).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid address hex '{}': {}", addr_hex, e),
        data: None,
    })?;

    let address = Address::from_slice(&addr_bytes).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid address length for '{}': {}", addr_hex, e),
        data: None,
    })?;

    let block_ref = params.get(1).and_then(|v| v.as_str()).map(String::from);

    let backend = backend.read().await;
    let account = backend
        .get_account(address, block_ref)
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Failed to get account: {}", e),
            data: None,
        })?;

    Ok(json!(account))
}

async fn handle_get_slot_number<B: RpcBackend>(
    backend: Arc<RwLock<B>>,
) -> Result<Value, JsonRpcError> {
    let backend = backend.read().await;
    let slot = backend.get_slot_number().map_err(|e| JsonRpcError {
        code: -32000,
        message: format!("Failed to get slot number: {}", e),
        data: None,
    })?;

    Ok(json!(slot))
}

async fn handle_get_finalized_slot<B: RpcBackend>(
    backend: Arc<RwLock<B>>,
) -> Result<Value, JsonRpcError> {
    let backend = backend.read().await;
    let slot = backend.get_finalized_slot().map_err(|e| JsonRpcError {
        code: -32000,
        message: format!("Failed to get finalized slot: {}", e),
        data: None,
    })?;

    Ok(json!(slot))
}

async fn handle_request_airdrop<B: RpcBackend>(
    params: &[Value],
    backend: Arc<RwLock<B>>,
) -> Result<Value, JsonRpcError> {
    if params.len() < 2 {
        return Err(JsonRpcError {
            code: -32602,
            message: "Missing parameters: [address, amount]".to_string(),
            data: None,
        });
    }

    // Airdrop is only available on devnet/testnet to prevent abuse
    let max_airdrop: u128 = 1_000_000_000_000; // 1M tokens max per request

    let addr_hex = params[0].as_str().ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!(
            "Invalid address: expected 0x-prefixed 40-char hex string, got {}",
            params[0]
        ),
        data: None,
    })?;
    let address = parse_address(addr_hex, "address")?;
    let amount = parse_u128_value(&params[1], "amount")?;
    if amount > max_airdrop {
        return Err(JsonRpcError {
            code: -32000,
            message: format!("airdrop amount {} exceeds maximum {}", amount, max_airdrop),
            data: None,
        });
    }

    let backend = backend.read().await;
    if !backend.allows_airdrop() {
        return Err(JsonRpcError {
            code: -32000,
            message: "airdrop is disabled on this network".to_string(),
            data: None,
        });
    }
    backend
        .request_airdrop(address, amount)
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Airdrop failed: {}", e),
            data: None,
        })?;

    Ok(json!({"success": true}))
}

async fn handle_health<B: RpcBackend>(
    backend: Arc<RwLock<B>>,
) -> Result<Value, JsonRpcError> {
    let backend = backend.read().await;
    let slot = backend.get_slot_number().unwrap_or(0);
    let finalized = backend.get_finalized_slot().unwrap_or(0);
    let peer_count = backend.get_peer_count().unwrap_or(0);
    let sync_status = backend
        .get_sync_status()
        .unwrap_or_else(|_| json!({"syncing": false}));
    Ok(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "latestSlot": slot,
        "finalizedSlot": finalized,
        "peerCount": peer_count,
        "sync": sync_status,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockBackend {
        allow_airdrop: bool,
    }

    impl RpcBackend for MockBackend {
        fn send_raw_transaction(&self, _tx_bytes: Vec<u8>) -> Result<H256> {
            Ok(H256::zero())
        }

        fn get_block_by_number(&self, _block_number: u64, _full_tx: bool) -> Result<Option<Block>> {
            Ok(None)
        }

        fn get_block_by_hash(&self, _block_hash: H256, _full_tx: bool) -> Result<Option<Block>> {
            Ok(None)
        }

        fn get_transaction_receipt(&self, _tx_hash: H256) -> Result<Option<TransactionReceipt>> {
            Ok(None)
        }

        fn get_state_root(&self, _block_ref: Option<String>) -> Result<H256> {
            Ok(H256::zero())
        }

        fn get_account(
            &self,
            _address: Address,
            _block_ref: Option<String>,
        ) -> Result<Option<Value>> {
            Ok(None)
        }

        fn get_slot_number(&self) -> Result<u64> {
            Ok(0)
        }

        fn get_finalized_slot(&self) -> Result<u64> {
            Ok(0)
        }

        fn allows_airdrop(&self) -> bool {
            self.allow_airdrop
        }

        fn request_airdrop(&self, _address: Address, _amount: u128) -> Result<()> {
            if self.allow_airdrop {
                Ok(())
            } else {
                Err(anyhow::anyhow!("airdrop not supported"))
            }
        }
    }

    #[tokio::test]
    async fn test_rpc_request_parsing() {
        let req_json = r#"{
            "jsonrpc": "2.0",
            "method": "aeth_getSlotNumber",
            "params": [],
            "id": 1
        }"#;

        let req: JsonRpcRequest = serde_json::from_str(req_json).unwrap();
        assert_eq!(req.method, "aeth_getSlotNumber");
        assert_eq!(req.jsonrpc, "2.0");
    }

    #[tokio::test]
    async fn test_get_slot_number() {
        let backend = Arc::new(RwLock::new(MockBackend::default()));
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "aeth_getSlotNumber".to_string(),
            params: vec![],
            id: json!(1),
        };

        let response = process_rpc_request(req, backend, 100_u64).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_send_transaction_payload() {
        let backend = Arc::new(RwLock::new(MockBackend::default()));
        let sender_pubkey = PublicKey::from_bytes(vec![7u8; 32]);
        let sender = sender_pubkey.to_address();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "aeth_sendTransaction".to_string(),
            params: vec![json!({
                "nonce": 1,
                "sender": format!("{:?}", sender),
                "sender_public_key": format!("0x{}", hex::encode(sender_pubkey.as_bytes())),
                "recipient": format!("0x{}", "11".repeat(20)),
                "amount": "1000",
                "fee": "2000000",
                "gas_limit": 500000,
                "memo": "rpc test",
                "reads": [],
                "writes": [],
                "signature": format!("0x{}", "22".repeat(64))
            })],
            id: json!(1),
        };

        let response = process_rpc_request(req, backend, 100_u64).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_airdrop_rejected_when_disabled() {
        let backend = Arc::new(RwLock::new(MockBackend::default()));
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "aeth_requestAirdrop".to_string(),
            params: vec![json!(format!("0x{}", "11".repeat(20))), json!("100")],
            id: json!(1),
        };

        let response = process_rpc_request(req, backend, 100_u64).await;
        let error = response.error.expect("airdrop should be rejected");
        assert!(error.message.contains("disabled on this network"));
    }

    #[tokio::test]
    async fn test_airdrop_allowed_when_backend_enables_it() {
        let backend = Arc::new(RwLock::new(MockBackend {
            allow_airdrop: true,
        }));
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "aeth_requestAirdrop".to_string(),
            params: vec![json!(format!("0x{}", "11".repeat(20))), json!("100")],
            id: json!(1),
        };

        let response = process_rpc_request(req, backend, 100_u64).await;
        assert!(response.error.is_none());
        assert_eq!(response.result, Some(json!({"success": true})));
    }

    #[tokio::test]
    async fn test_chain_id_returns_configured_value() {
        const TESTNET_CHAIN_ID: u64 = 100;
        let backend = Arc::new(RwLock::new(MockBackend::default()));
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "aeth_chainId".to_string(),
            params: vec![],
            id: json!(1),
        };

        let response = process_rpc_request(req.clone(), backend.clone(), TESTNET_CHAIN_ID).await;
        assert!(response.error.is_none());
        // TESTNET_CHAIN_ID = 100 = 0x64
        assert_eq!(response.result, Some(json!("0x64")));

        // A different chain_id returns a different result
        let response2 = process_rpc_request(req, backend, 1).await;
        assert_eq!(response2.result, Some(json!("0x1")));
    }

    #[tokio::test]
    async fn test_send_transaction_uses_server_chain_id() {
        const TESTNET_CHAIN_ID: u64 = 100;
        let backend = Arc::new(RwLock::new(MockBackend::default()));
        let sender_pubkey = PublicKey::from_bytes(vec![7u8; 32]);
        let sender = sender_pubkey.to_address();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "aeth_sendTransaction".to_string(),
            params: vec![json!({
                "nonce": 0,
                "sender": format!("{:?}", sender),
                "sender_public_key": format!("0x{}", hex::encode(sender_pubkey.as_bytes())),
                "recipient": format!("0x{}", "11".repeat(20)),
                "amount": "500",
                "fee": "1000000",
                "gas_limit": 21000,
                "reads": [],
                "writes": [],
                "signature": format!("0x{}", "aa".repeat(64))
            })],
            id: json!(2),
        };
        // Both mainnet and testnet chain_ids should produce a successful RPC response
        // (MockBackend accepts all; the chain_id is stamped, not re-validated here)
        let response = process_rpc_request(req, backend, TESTNET_CHAIN_ID).await;
        // MockBackend::send_raw_transaction returns Ok so result should be present
        assert!(response.error.is_none());
        assert!(response.result.is_some());
    }

    #[tokio::test]
    async fn test_health_endpoint_returns_node_status() {
        let backend = Arc::new(RwLock::new(MockBackend::default()));
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "aeth_health".to_string(),
            params: vec![],
            id: json!(1),
        };

        let response = process_rpc_request(req, backend, 100_u64).await;
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert_eq!(result["status"], "ok");
        assert_eq!(result["latestSlot"], 0);
        assert_eq!(result["finalizedSlot"], 0);
        assert_eq!(result["peerCount"], 0);
        assert_eq!(result["sync"]["syncing"], false);
    }
}
