use serde::{Deserialize, Serialize};

use aether_types::{Address, H256};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientConfig {
    pub default_fee: u128,
    pub default_gas_limit: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            default_fee: 2_000_000,
            default_gas_limit: 500_000,
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
