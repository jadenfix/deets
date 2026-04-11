use serde::{Deserialize, Serialize};
use serde_json::Value;

use aether_types::{Address, H256};

/// Summary of a block as returned by `aeth_getBlockByHash` / `aeth_getBlockByNumber`.
/// Fields mirror the server's JSON serialization of `aether_types::Block`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcBlock {
    pub hash: H256,
    pub parent_hash: H256,
    pub slot: u64,
    pub proposer: Address,
    #[serde(default)]
    pub transactions: Vec<Value>,
}

/// Transaction receipt as returned by `aeth_getTransactionReceipt`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcReceipt {
    pub tx_hash: H256,
    pub block_hash: H256,
    pub slot: u64,
    pub status: Value,
    pub gas_used: u64,
}

/// Account state as returned by `aeth_getAccount`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcAccount {
    pub address: Address,
    pub balance: u128,
    pub nonce: u64,
    #[serde(default)]
    pub stake: u128,
}

/// Node health response from `aeth_health`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeHealth {
    /// `"ok"` when fully synced, `"syncing"` when catching up.
    pub status: String,
    pub version: String,
    #[serde(rename = "latestSlot")]
    pub latest_slot: u64,
    #[serde(rename = "finalizedSlot")]
    pub finalized_slot: u64,
    #[serde(rename = "peerCount")]
    pub peer_count: usize,
    pub sync: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientConfig {
    pub default_fee: u128,
    pub default_gas_limit: u64,
    /// Timeout in seconds applied individually to each I/O phase (connect, write, read).
    /// Defaults to 30 s. Use `#[serde(default)]` so existing serialized configs remain valid.
    #[serde(default = "default_timeout_secs")]
    pub request_timeout_secs: u64,
}

fn default_timeout_secs() -> u64 {
    30
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            default_fee: 2_000_000,
            default_gas_limit: 500_000,
            request_timeout_secs: default_timeout_secs(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferRequest {
    pub recipient: Address,
    pub amount: u128,
    pub memo: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubmitResponse {
    pub tx_hash: H256,
    pub accepted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobRequest {
    pub job_id: String,
    pub model_hash: H256,
    pub input_hash: H256,
    pub max_fee: u128,
    pub expires_at: u64,
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobSubmission {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: JobRequest,
}
