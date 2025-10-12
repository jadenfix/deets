use crate::primitives::{H256, Address};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub address: Address,
    pub balance: u128,
    pub nonce: u64,
    pub code_hash: Option<H256>,
    pub storage_root: H256,
}

impl Account {
    pub fn new(address: Address) -> Self {
        Account {
            address,
            balance: 0,
            nonce: 0,
            code_hash: None,
            storage_root: H256::zero(),
        }
    }

    pub fn with_balance(address: Address, balance: u128) -> Self {
        Account {
            address,
            balance,
            nonce: 0,
            code_hash: None,
            storage_root: H256::zero(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Utxo {
    pub amount: u128,
    pub owner: Address,
    pub script_hash: Option<H256>,
}

