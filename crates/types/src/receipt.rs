use crate::{hash::H256, Address};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionStatus {
    Success,
    Failure { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub tx_hash: H256,
    pub block_hash: H256,
    pub slot: u64,
    pub status: ExecutionStatus,
    pub gas_used: u64,
    pub fee_paid: u128,
    pub logs: Vec<Log>,
    pub state_root: H256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    pub address: Address,
    pub topics: Vec<H256>,
    pub data: Vec<u8>,
}

impl Receipt {
    pub fn success(
        tx_hash: H256,
        block_hash: H256,
        slot: u64,
        gas_used: u64,
        fee_paid: u128,
        state_root: H256,
    ) -> Self {
        Self {
            tx_hash,
            block_hash,
            slot,
            status: ExecutionStatus::Success,
            gas_used,
            fee_paid,
            logs: Vec::new(),
            state_root,
        }
    }

    pub fn failure(
        tx_hash: H256,
        block_hash: H256,
        slot: u64,
        gas_used: u64,
        fee_paid: u128,
        error: String,
        state_root: H256,
    ) -> Self {
        Self {
            tx_hash,
            block_hash,
            slot,
            status: ExecutionStatus::Failure { error },
            gas_used,
            fee_paid,
            logs: Vec::new(),
            state_root,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self.status, ExecutionStatus::Success)
    }
}

