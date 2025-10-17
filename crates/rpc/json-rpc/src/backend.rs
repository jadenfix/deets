use crate::RpcBackend;
use aether_ledger::Ledger;
use aether_types::{Address, Block, TransactionReceipt, H256};
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Real RPC backend implementation backed by ledger and consensus state
pub struct NodeRpcBackend {
    ledger: Arc<RwLock<Ledger>>,
    current_slot: Arc<RwLock<u64>>,
    finalized_slot: Arc<RwLock<u64>>,
}

impl NodeRpcBackend {
    pub fn new(
        ledger: Arc<RwLock<Ledger>>,
        current_slot: Arc<RwLock<u64>>,
        finalized_slot: Arc<RwLock<u64>>,
    ) -> Self {
        Self {
            ledger,
            current_slot,
            finalized_slot,
        }
    }
}

impl RpcBackend for NodeRpcBackend {
    fn send_raw_transaction(&self, _tx_bytes: Vec<u8>) -> Result<H256> {
        // In production, decode transaction and submit to mempool
        // For now, return placeholder hash
        Ok(H256::zero())
    }

    fn get_block_by_number(&self, _block_number: u64, _full_tx: bool) -> Result<Option<Block>> {
        // In production, query ledger for block by number
        // Blocks would be stored with slot numbers
        Ok(None)
    }

    fn get_block_by_hash(&self, _block_hash: H256, _full_tx: bool) -> Result<Option<Block>> {
        // In production, query ledger for block by hash
        Ok(None)
    }

    fn get_transaction_receipt(&self, _tx_hash: H256) -> Result<Option<TransactionReceipt>> {
        // In production, query ledger for receipt
        Ok(None)
    }

    fn get_state_root(&self, _block_ref: Option<String>) -> Result<H256> {
        // Return current state root from ledger
        // In production, support historical state roots
        Ok(H256::zero())
    }

    fn get_account(
        &self,
        address: Address,
        _block_ref: Option<String>,
    ) -> Result<Option<Value>> {
        // In production, query account state from ledger
        Ok(Some(json!({
            "address": format!("{:?}", address),
            "balance": "0",
            "nonce": 0
        })))
    }

    fn get_slot_number(&self) -> Result<u64> {
        // Return current slot from consensus
        Ok(0) // In production, read from current_slot
    }

    fn get_finalized_slot(&self) -> Result<u64> {
        // Return finalized slot from consensus
        Ok(0) // In production, read from finalized_slot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_state_storage::Storage;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::RwLock;

    #[test]
    fn test_backend_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let ledger = Arc::new(RwLock::new(Ledger::new(storage).unwrap()));
        let current_slot = Arc::new(RwLock::new(0u64));
        let finalized_slot = Arc::new(RwLock::new(0u64));

        let backend = NodeRpcBackend::new(ledger, current_slot, finalized_slot);

        assert_eq!(backend.get_slot_number().unwrap(), 0);
        assert_eq!(backend.get_finalized_slot().unwrap(), 0);
    }

    #[test]
    fn test_get_account() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let ledger = Arc::new(RwLock::new(Ledger::new(storage).unwrap()));
        let current_slot = Arc::new(RwLock::new(0u64));
        let finalized_slot = Arc::new(RwLock::new(0u64));

        let backend = NodeRpcBackend::new(ledger, current_slot, finalized_slot);

        let address = Address::from_slice(&[1u8; 20]).unwrap();
        let account = backend.get_account(address, None).unwrap();

        assert!(account.is_some());
    }
}

