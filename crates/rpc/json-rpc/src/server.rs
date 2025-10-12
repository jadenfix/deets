use aether_types::{Address, Block, TransactionReceipt, H256};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
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

pub trait RpcBackend: Send + Sync {
    fn send_raw_transaction(&self, tx_bytes: Vec<u8>) -> Result<H256>;
    fn get_block_by_number(&self, block_number: u64, full_tx: bool) -> Result<Option<Block>>;
    fn get_block_by_hash(&self, block_hash: H256, full_tx: bool) -> Result<Option<Block>>;
    fn get_transaction_receipt(&self, tx_hash: H256) -> Result<Option<TransactionReceipt>>;
    fn get_state_root(&self, block_ref: Option<String>) -> Result<H256>;
    fn get_account(&self, address: Address, block_ref: Option<String>) -> Result<Option<Value>>;
    fn get_slot_number(&self) -> Result<u64>;
    fn get_finalized_slot(&self) -> Result<u64>;
}

pub struct JsonRpcServer<B: RpcBackend> {
    backend: Arc<RwLock<B>>,
    port: u16,
}

impl<B: RpcBackend + 'static> JsonRpcServer<B> {
    pub fn new(backend: B, port: u16) -> Self {
        Self {
            backend: Arc::new(RwLock::new(backend)),
            port,
        }
    }

    pub async fn run(self) -> Result<()> {
        let backend = self.backend.clone();

        let rpc = warp::post()
            .and(warp::path::end())
            .and(warp::body::json())
            .and(with_backend(backend))
            .and_then(handle_rpc_request);

        let health = warp::get()
            .and(warp::path("health"))
            .map(|| warp::reply::json(&json!({"status": "ok"})));

        let routes = rpc.or(health);

        println!("JSON-RPC server listening on 127.0.0.1:{}", self.port);
        warp::serve(routes).run(([127, 0, 0, 1], self.port)).await;

        Ok(())
    }
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
        "aeth_getBlockByNumber" => handle_get_block_by_number(&req.params, backend).await,
        "aeth_getBlockByHash" => handle_get_block_by_hash(&req.params, backend).await,
        "aeth_getTransactionReceipt" => handle_get_transaction_receipt(&req.params, backend).await,
        "aeth_getStateRoot" => handle_get_state_root(&req.params, backend).await,
        "aeth_getAccount" => handle_get_account(&req.params, backend).await,
        "aeth_getSlotNumber" => handle_get_slot_number(backend).await,
        "aeth_getFinalizedSlot" => handle_get_finalized_slot(backend).await,
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
        backend.get_slot_number().map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Failed to get latest slot: {}", e),
            data: None,
        })?
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
}
