use aether_types::{
    Address, Block, PublicKey, Signature, Transaction, TransactionReceipt, TransferPayload, H256,
    TRANSFER_PROGRAM_ID,
};
use anyhow::Result;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::net::IpAddr;
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
                "hash": format!("{:?}", block.hash()),
                "proposer": format!("{:?}", block.header.proposer),
                "txCount": block.transactions.len(),
                "timestamp": block.header.timestamp,
            }),
        };
        let _ = self.sender.send(event);
    }

    /// Broadcast a finality event.
    pub fn notify_finality(&self, slot: u64, block_hash: H256) {
        let event = SubscriptionEvent {
            topic: "finality".to_string(),
            data: json!({
                "finalizedSlot": slot,
                "blockHash": format!("{:?}", block_hash),
            }),
        };
        let _ = self.sender.send(event);
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
    bind_addr: IpAddr,
    port: u16,
}

impl<B: RpcBackend + 'static> JsonRpcServer<B> {
    pub fn new(backend: B, port: u16) -> Self {
        Self::new_with_bind_addr(backend, "127.0.0.1".parse().expect("valid loopback"), port)
    }

    pub fn new_with_bind_addr(backend: B, bind_addr: IpAddr, port: u16) -> Self {
        Self {
            backend: Arc::new(RwLock::new(backend)),
            subscriptions: Arc::new(SubscriptionManager::new()),
            bind_addr,
            port,
        }
    }

    /// Get a reference to the subscription manager for event broadcasting.
    pub fn subscription_manager(&self) -> Arc<SubscriptionManager> {
        self.subscriptions.clone()
    }

    pub async fn run(self) -> Result<()> {
        let backend = self.backend.clone();
        let subs = self.subscriptions.clone();

        let rpc = warp::post()
            .and(warp::path::end())
            .and(warp::body::content_length_limit(1024 * 256)) // 256KB max
            .and(warp::body::json())
            .and(with_backend(backend))
            .and_then(handle_rpc_request);

        let health = warp::get()
            .and(warp::path("health"))
            .map(|| warp::reply::json(&json!({"status": "ok"})));

        // WebSocket subscription endpoint
        let ws_subs = subs.clone();
        let ws = warp::path("ws")
            .and(warp::ws())
            .map(move |ws: warp::ws::Ws| {
                let subs = ws_subs.clone();
                ws.on_upgrade(move |socket| handle_ws_connection(socket, subs))
            });

        let cors = warp::cors()
            .allow_origins(vec!["http://localhost:3000", "http://127.0.0.1:3000"])
            .allow_methods(vec!["POST", "GET", "OPTIONS"])
            .allow_headers(vec!["content-type"]);

        let routes = rpc.or(health).or(ws).with(cors);

        println!(
            "JSON-RPC server listening on {}:{}",
            self.bind_addr, self.port
        );
        println!(
            "WebSocket subscriptions on ws://{}:{}/ws",
            self.bind_addr, self.port
        );
        warp::serve(routes).run((self.bind_addr, self.port)).await;

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
                    let msg = serde_json::to_string(&event).unwrap_or_default();
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

async fn handle_rpc_request<B: RpcBackend>(
    req: JsonRpcRequest,
    backend: Arc<RwLock<B>>,
) -> Result<impl Reply, warp::Rejection> {
    let response = process_rpc_request(req.clone(), backend).await;
    Ok(warp::reply::json(&response))
}

async fn process_rpc_request<B: RpcBackend>(
    req: JsonRpcRequest,
    backend: Arc<RwLock<B>>,
) -> JsonRpcResponse {
    let result = match req.method.as_str() {
        "aeth_sendRawTransaction" => handle_send_raw_transaction(&req.params, backend).await,
        "aeth_sendTransaction" => handle_send_transaction(&req.params, backend).await,
        "aeth_getBlockByNumber" => handle_get_block_by_number(&req.params, backend).await,
        "aeth_getBlockByHash" => handle_get_block_by_hash(&req.params, backend).await,
        "aeth_getTransactionReceipt" => handle_get_transaction_receipt(&req.params, backend).await,
        "aeth_getStateRoot" => handle_get_state_root(&req.params, backend).await,
        "aeth_getAccount" => handle_get_account(&req.params, backend).await,
        "aeth_getSlotNumber" => handle_get_slot_number(backend).await,
        "aeth_getFinalizedSlot" => handle_get_finalized_slot(backend).await,
        "aeth_requestAirdrop" => handle_request_airdrop(&req.params, backend).await,
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
        message: "Invalid parameter type".to_string(),
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

    Ok(json!(format!("{:?}", tx_hash)))
}

async fn handle_send_transaction<B: RpcBackend>(
    params: &[Value],
    backend: Arc<RwLock<B>>,
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
        chain_id: 1,
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

    Ok(json!(format!("{:?}", tx_hash)))
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
        message: "Invalid hash".to_string(),
        data: None,
    })?;

    let full_tx = params[1].as_bool().unwrap_or(false);

    let hash_bytes = hex::decode(hash_hex.trim_start_matches("0x")).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid hex: {}", e),
        data: None,
    })?;

    let block_hash = H256::from_slice(&hash_bytes).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid hash length: {}", e),
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
        message: "Invalid hash".to_string(),
        data: None,
    })?;

    let hash_bytes = hex::decode(hash_hex.trim_start_matches("0x")).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid hex: {}", e),
        data: None,
    })?;

    let tx_hash = H256::from_slice(&hash_bytes).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid hash length: {}", e),
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

    Ok(json!(format!("{:?}", state_root)))
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
        message: "Invalid address".to_string(),
        data: None,
    })?;

    let addr_bytes = hex::decode(addr_hex.trim_start_matches("0x")).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid hex: {}", e),
        data: None,
    })?;

    let address = Address::from_slice(&addr_bytes).map_err(|e| JsonRpcError {
        code: -32602,
        message: format!("Invalid address length: {}", e),
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

    let addr_hex = params[0].as_str().ok_or_else(|| JsonRpcError {
        code: -32602,
        message: "Invalid address".to_string(),
        data: None,
    })?;
    let address = parse_address(addr_hex, "address")?;
    let amount = parse_u128_value(&params[1], "amount")?;

    let backend = backend.read().await;
    backend
        .request_airdrop(address, amount)
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Airdrop failed: {}", e),
            data: None,
        })?;

    Ok(json!({"success": true}))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBackend;

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
        let backend = Arc::new(RwLock::new(MockBackend));
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "aeth_getSlotNumber".to_string(),
            params: vec![],
            id: json!(1),
        };

        let response = process_rpc_request(req, backend).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_send_transaction_payload() {
        let backend = Arc::new(RwLock::new(MockBackend));
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

        let response = process_rpc_request(req, backend).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }
}
