use aether_types::{Address, Transaction, H256};
use anyhow::Result;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

const MAX_MEMPOOL_SIZE: usize = 50_000;
const MIN_FEE: u128 = 1000;

#[derive(Clone)]
struct PrioritizedTx {
    tx: Transaction,
    fee_rate: u128,
    timestamp: u64,
}

impl PartialEq for PrioritizedTx {
    fn eq(&self, other: &Self) -> bool {
        self.tx.hash() == other.tx.hash()
    }
}

impl Eq for PrioritizedTx {}

impl PartialOrd for PrioritizedTx {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedTx {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher fee rate = higher priority
        match self.fee_rate.cmp(&other.fee_rate) {
            Ordering::Equal => {
                // Earlier timestamp = higher priority (FIFO for same fee)
                other.timestamp.cmp(&self.timestamp)
            }
            other => other,
        }
    }
}

pub struct Mempool {
    pending: BinaryHeap<PrioritizedTx>,
    by_hash: HashMap<H256, Transaction>,
    by_sender: HashMap<Address, HashSet<H256>>,
    current_time: u64,
}

impl Mempool {
    pub fn new() -> Self {
        Mempool {
            pending: BinaryHeap::new(),
            by_hash: HashMap::new(),
            by_sender: HashMap::new(),
            current_time: 0,
        }
    }

    pub fn add_transaction(&mut self, tx: Transaction) -> Result<()> {
        tx.verify_signature()
            .map_err(|e| anyhow::anyhow!("invalid signature: {}", e))?;

        tx.calculate_fee()
            .map_err(|e| anyhow::anyhow!("invalid fee: {}", e))?;

        let tx_hash = tx.hash();

        // Check if already in pool
        if self.by_hash.contains_key(&tx_hash) {
            // Check for replace-by-fee
            let existing = self.by_hash.get(&tx_hash).unwrap();
            if tx.fee <= existing.fee + (existing.fee / 10) {
                anyhow::bail!("fee not high enough to replace (need 10% increase)");
            }

            // Remove old transaction
            self.by_hash.remove(&tx_hash);
            if let Some(sender_txs) = self.by_sender.get_mut(&tx.sender) {
                sender_txs.remove(&tx_hash);
            }
        }

        // Validate minimum fee
        if tx.fee < MIN_FEE {
            anyhow::bail!("fee below minimum");
        }

        // Check mempool capacity
        if self.by_hash.len() >= MAX_MEMPOOL_SIZE {
            // Evict lowest fee transaction
            self.evict_lowest_fee();
        }

        // Calculate fee rate
        let tx_size = bincode::serialize(&tx).unwrap().len() as u128;
        let fee_rate = if tx_size > 0 {
            tx.fee / tx_size
        } else {
            tx.fee
        };

        // Add to structures
        self.by_hash.insert(tx_hash, tx.clone());
        self.by_sender.entry(tx.sender).or_default().insert(tx_hash);

        self.pending.push(PrioritizedTx {
            tx,
            fee_rate,
            timestamp: self.current_time,
        });

        self.current_time += 1;

        Ok(())
    }

    pub fn get_transactions(&mut self, max_count: usize, max_gas: u64) -> Vec<Transaction> {
        let mut selected = Vec::new();
        let mut total_gas = 0u64;
        let mut temp_heap = BinaryHeap::new();

        while let Some(ptx) = self.pending.pop() {
            if selected.len() >= max_count || total_gas >= max_gas {
                temp_heap.push(ptx);
                break;
            }

            if total_gas + ptx.tx.gas_limit <= max_gas {
                let tx_hash = ptx.tx.hash();
                if self.by_hash.contains_key(&tx_hash) {
                    selected.push(ptx.tx.clone());
                    total_gas += ptx.tx.gas_limit;
                }
            } else {
                temp_heap.push(ptx);
            }
        }

        // Return unselected transactions to heap
        while let Some(ptx) = temp_heap.pop() {
            self.pending.push(ptx);
        }

        selected
    }

    pub fn remove_transactions(&mut self, tx_hashes: &[H256]) {
        for hash in tx_hashes {
            if let Some(tx) = self.by_hash.remove(hash) {
                if let Some(sender_txs) = self.by_sender.get_mut(&tx.sender) {
                    sender_txs.remove(hash);
                }
            }
        }

        // Rebuild heap without removed transactions
        self.rebuild_heap();
    }

    fn rebuild_heap(&mut self) {
        let mut new_heap = BinaryHeap::new();

        while let Some(ptx) = self.pending.pop() {
            let tx_hash = ptx.tx.hash();
            if self.by_hash.contains_key(&tx_hash) {
                new_heap.push(ptx);
            }
        }

        self.pending = new_heap;
    }

    fn evict_lowest_fee(&mut self) {
        // Convert heap to vec, sort, remove lowest
        let mut txs: Vec<_> = std::mem::take(&mut self.pending).into_vec();
        txs.sort_by(|a, b| b.cmp(a)); // Reverse sort (lowest last)

        if let Some(lowest) = txs.pop() {
            let tx_hash = lowest.tx.hash();
            self.by_hash.remove(&tx_hash);
            if let Some(sender_txs) = self.by_sender.get_mut(&lowest.tx.sender) {
                sender_txs.remove(&tx_hash);
            }
        }

        self.pending = txs.into();
    }

    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::{PublicKey, Signature};
    use std::collections::HashSet;

    fn create_test_tx(nonce: u64, fee: u128) -> Transaction {
        let pubkey_bytes = vec![(nonce as u8).saturating_add(1); 32];
        let sender_pubkey = PublicKey::from_bytes(pubkey_bytes);
        let sender = sender_pubkey.to_address();
        Transaction {
            nonce,
            sender,
            sender_pubkey,
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21000,
            fee,
            signature: Signature::from_bytes(vec![0; 64]),
        }
    }

    #[test]
    fn test_add_transaction() {
        let mut mempool = Mempool::new();
        let tx = create_test_tx(0, 60_000);

        mempool.add_transaction(tx).unwrap();
        assert_eq!(mempool.len(), 1);
    }

    #[test]
    fn test_priority_ordering() {
        let mut mempool = Mempool::new();

        let tx1 = create_test_tx(0, 110_000);
        let tx2 = create_test_tx(1, 160_000);
        let tx3 = create_test_tx(2, 130_000);

        mempool.add_transaction(tx1).unwrap();
        mempool.add_transaction(tx2).unwrap();
        mempool.add_transaction(tx3).unwrap();

        let txs = mempool.get_transactions(10, 1_000_000);

        // Should be ordered by fee: tx2, tx3, tx1
        assert_eq!(txs[0].fee, 160_000);
        assert_eq!(txs[1].fee, 130_000);
        assert_eq!(txs[2].fee, 110_000);
    }

    #[test]
    fn test_gas_limit() {
        let mut mempool = Mempool::new();

        let tx1 = create_test_tx(0, 90_000);
        let tx2 = create_test_tx(1, 120_000);

        mempool.add_transaction(tx1).unwrap();
        mempool.add_transaction(tx2).unwrap();

        let txs = mempool.get_transactions(10, 25000); // Only enough for 1 tx
        assert_eq!(txs.len(), 1);
    }

    #[test]
    fn test_remove_transactions() {
        let mut mempool = Mempool::new();

        let tx1 = create_test_tx(0, 90_000);
        let tx2 = create_test_tx(1, 120_000);

        mempool.add_transaction(tx1.clone()).unwrap();
        mempool.add_transaction(tx2.clone()).unwrap();

        let hashes = vec![tx1.hash()];
        mempool.remove_transactions(&hashes);

        assert_eq!(mempool.len(), 1);
    }
}
