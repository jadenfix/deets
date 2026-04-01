use aether_types::{Address, FeeParams, Transaction, H256};
use anyhow::Result;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::time::Instant;

const MAX_MEMPOOL_SIZE: usize = 50_000;
const MIN_FEE: u128 = 1000;
const MAX_TXS_PER_SENDER_PER_SECOND: u32 = 100;
const RATE_LIMIT_WINDOW_SECS: u64 = 1;
/// Txs waiting longer than this many slots with sufficient fee must be included.
const FORCED_INCLUSION_SLOTS: u64 = 10;

#[derive(Clone)]
struct PrioritizedTx {
    tx: Transaction,
    fee_rate: u128,
    timestamp: u64,
    /// Slot when the tx entered the mempool (for forced inclusion tracking).
    submitted_slot: u64,
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
        match self.fee_rate.cmp(&other.fee_rate) {
            Ordering::Equal => other.timestamp.cmp(&self.timestamp),
            other => other,
        }
    }
}

/// Rate limit tracker per sender.
struct RateLimitEntry {
    window_start: Instant,
    count: u32,
}

pub struct Mempool {
    /// Priority queue for pending (ready-to-execute) transactions.
    pending: BinaryHeap<PrioritizedTx>,
    /// Quick lookup by hash.
    by_hash: HashMap<H256, Transaction>,
    /// Txs grouped by sender.
    by_sender: HashMap<Address, HashSet<H256>>,
    /// Next expected nonce per sender (from chain state).
    next_nonce: HashMap<Address, u64>,
    /// Queued transactions: future nonces waiting for gaps to fill.
    /// sender → nonce → Transaction
    queued: HashMap<Address, BTreeMap<u64, Transaction>>,
    /// Per-sender rate limiting.
    rate_limits: HashMap<Address, RateLimitEntry>,
    /// Monotonic counter for FIFO tiebreaking.
    current_time: u64,
    /// Current slot number (updated externally for forced inclusion tracking).
    current_slot: u64,
    /// Fee parameters for validating transaction fees.
    fee_params: FeeParams,
}

impl Mempool {
    pub fn new(fee_params: FeeParams) -> Self {
        Mempool {
            pending: BinaryHeap::new(),
            by_hash: HashMap::new(),
            by_sender: HashMap::new(),
            next_nonce: HashMap::new(),
            queued: HashMap::new(),
            rate_limits: HashMap::new(),
            current_time: 0,
            current_slot: 0,
            fee_params,
        }
    }

    /// Create with devnet fee defaults (convenience for tests).
    pub fn with_defaults() -> Self {
        Self::new(aether_types::ChainConfig::devnet().fees)
    }

    /// Update the current slot (for forced inclusion age tracking).
    pub fn set_current_slot(&mut self, slot: u64) {
        self.current_slot = slot;
    }

    /// Set the expected next nonce for a sender (from chain state).
    pub fn set_sender_nonce(&mut self, sender: Address, nonce: u64) {
        self.next_nonce.insert(sender, nonce);
    }

    /// Add a transaction to the mempool with nonce ordering and rate limiting.
    pub fn add_transaction(&mut self, tx: Transaction) -> Result<()> {
        tx.verify_signature()
            .map_err(|e| anyhow::anyhow!("invalid signature: {}", e))?;

        tx.calculate_fee(&self.fee_params)
            .map_err(|e| anyhow::anyhow!("invalid fee: {}", e))?;

        if tx.fee < MIN_FEE {
            anyhow::bail!("fee below minimum");
        }

        // Rate limiting
        self.check_rate_limit(&tx.sender)?;

        let tx_hash = tx.hash();

        // Exact duplicate check
        if self.by_hash.contains_key(&tx_hash) {
            anyhow::bail!("duplicate transaction");
        }

        // Replace-by-fee: if the same sender already has a tx with the same nonce,
        // allow replacement only if the new fee is >10% higher.
        if let Some(existing_hashes) = self.by_sender.get(&tx.sender) {
            let same_nonce_hash = existing_hashes
                .iter()
                .find(|h| self.by_hash.get(h).map_or(false, |t| t.nonce == tx.nonce))
                .copied();
            if let Some(old_hash) = same_nonce_hash {
                let old_fee = self.by_hash[&old_hash].fee;
                let min_replacement_fee = old_fee.saturating_add(old_fee / 10);
                if tx.fee <= min_replacement_fee {
                    anyhow::bail!(
                        "fee {} not high enough to replace (need >10% above {})",
                        tx.fee,
                        old_fee
                    );
                }
                let old_nonce = self.by_hash[&old_hash].nonce;
                // Remove the old transaction being replaced
                self.by_hash.remove(&old_hash);
                if let Some(sender_txs) = self.by_sender.get_mut(&tx.sender) {
                    sender_txs.remove(&old_hash);
                }
                // If the replaced tx was already pending (nonce < next_nonce),
                // roll back next_nonce so the replacement can enter pending.
                let expected = self.next_nonce.get(&tx.sender).copied().unwrap_or(0);
                if old_nonce < expected {
                    self.next_nonce.insert(tx.sender, old_nonce);
                }
                // Stale heap entry is harmless — skipped in get_transactions() via by_hash check
            }
        }

        // Capacity check
        if self.by_hash.len() >= MAX_MEMPOOL_SIZE {
            self.evict_lowest_fee();
        }

        // Nonce-based routing
        let expected_nonce = self.next_nonce.get(&tx.sender).copied().unwrap_or(0);

        if tx.nonce < expected_nonce {
            anyhow::bail!(
                "nonce too low: tx nonce {} < expected {}",
                tx.nonce,
                expected_nonce
            );
        }

        // Track in by_hash and by_sender
        self.by_hash.insert(tx_hash, tx.clone());
        self.by_sender.entry(tx.sender).or_default().insert(tx_hash);

        if tx.nonce == expected_nonce {
            // Ready to execute — add to pending
            self.add_to_pending(tx);
            // Promote any queued txs that are now sequential
            self.promote_queued(tx_hash);
        } else {
            // Future nonce — queue it
            self.queued
                .entry(tx.sender)
                .or_default()
                .insert(tx.nonce, tx);
        }

        Ok(())
    }

    /// Promote queued transactions that are now sequential after a nonce advancement.
    fn promote_queued(&mut self, _trigger_hash: H256) {
        // Collect senders that might have promotable txs
        let senders: Vec<Address> = self.queued.keys().cloned().collect();

        for sender in senders {
            loop {
                let expected = self.next_nonce.get(&sender).copied().unwrap_or(0);
                let should_promote = self
                    .queued
                    .get(&sender)
                    .and_then(|q| q.get(&expected))
                    .is_some();

                if !should_promote {
                    break;
                }

                let tx = self
                    .queued
                    .get_mut(&sender)
                    .unwrap()
                    .remove(&expected)
                    .unwrap();

                self.add_to_pending(tx);

                // Clean up empty queued maps
                if self.queued.get(&sender).map_or(true, |q| q.is_empty()) {
                    self.queued.remove(&sender);
                }
            }
        }
    }

    fn add_to_pending(&mut self, tx: Transaction) {
        let tx_size = bincode::serialize(&tx)
            .map(|b| b.len() as u128)
            .unwrap_or(1); // Fallback to 1 to avoid divide-by-zero
        let fee_rate = if tx_size > 0 {
            tx.fee / tx_size
        } else {
            tx.fee
        };

        // Advance expected nonce
        let sender = tx.sender;
        let next = tx.nonce + 1;
        let current_expected = self.next_nonce.get(&sender).copied().unwrap_or(0);
        if next > current_expected {
            self.next_nonce.insert(sender, next);
        }

        self.pending.push(PrioritizedTx {
            tx,
            fee_rate,
            timestamp: self.current_time,
            submitted_slot: self.current_slot,
        });
        self.current_time += 1;
    }

    /// Handle a chain reorg: re-add reverted txs, remove invalid ones.
    pub fn reorg(&mut self, reverted_txs: Vec<Transaction>, new_tip_nonces: HashMap<Address, u64>) {
        // Reset nonces to the new chain tip
        for (sender, nonce) in &new_tip_nonces {
            self.next_nonce.insert(*sender, *nonce);
        }

        // Remove any pending txs with nonces below the new chain tip
        // (these were already executed in the surviving chain)
        let mut stale_hashes = Vec::new();
        for (hash, tx) in &self.by_hash {
            let tip_nonce = new_tip_nonces.get(&tx.sender).copied().unwrap_or(0);
            if tx.nonce < tip_nonce {
                stale_hashes.push(*hash);
            }
        }
        self.remove_transactions(&stale_hashes);

        // Clear rate limits during reorg (reverted txs are legitimate)
        self.rate_limits.clear();

        // Re-add reverted transactions (they're no longer in a block)
        for tx in reverted_txs {
            if let Err(e) = self.add_transaction(tx) {
                // Log at module level since tracing may not be available
                eprintln!("failed to re-add reverted tx during reorg: {e}");
            }
        }
    }

    fn check_rate_limit(&mut self, sender: &Address) -> Result<()> {
        let now = Instant::now();

        // Prune stale rate limit entries (older than 60s) to bound memory
        if self.rate_limits.len() > 10_000 {
            self.rate_limits
                .retain(|_, e| now.duration_since(e.window_start).as_secs() < 60);
        }

        let entry = self.rate_limits.entry(*sender).or_insert(RateLimitEntry {
            window_start: now,
            count: 0,
        });

        if now.duration_since(entry.window_start).as_secs() >= RATE_LIMIT_WINDOW_SECS {
            entry.window_start = now;
            entry.count = 1;
            Ok(())
        } else {
            entry.count += 1;
            if entry.count > MAX_TXS_PER_SENDER_PER_SECOND {
                anyhow::bail!("rate limited: too many transactions from sender");
            }
            Ok(())
        }
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

        while let Some(ptx) = temp_heap.pop() {
            self.pending.push(ptx);
        }

        selected
    }

    /// Return transactions that MUST be included (anti-censorship).
    /// A tx must be included if it has waited > FORCED_INCLUSION_SLOTS
    /// and pays >= 2x the base_fee (clearly willing to pay market rate).
    pub fn must_include_transactions(&self, current_slot: u64, base_fee: u128) -> Vec<Transaction> {
        let min_fee = base_fee.saturating_mul(2);
        let mut forced = Vec::new();

        // Iterate pending heap without consuming (peek at all items)
        for ptx in self.pending.iter() {
            let age = current_slot.saturating_sub(ptx.submitted_slot);
            if age >= FORCED_INCLUSION_SLOTS && ptx.tx.fee >= min_fee {
                forced.push(ptx.tx.clone());
            }
        }

        forced
    }

    pub fn remove_transactions(&mut self, tx_hashes: &[H256]) {
        for hash in tx_hashes {
            if let Some(tx) = self.by_hash.remove(hash) {
                if let Some(sender_txs) = self.by_sender.get_mut(&tx.sender) {
                    sender_txs.remove(hash);
                }
            }
        }
        self.rebuild_heap();
    }

    fn rebuild_heap(&mut self) {
        let mut new_heap = BinaryHeap::new();
        while let Some(ptx) = self.pending.pop() {
            if self.by_hash.contains_key(&ptx.tx.hash()) {
                new_heap.push(ptx);
            }
        }
        self.pending = new_heap;
    }

    fn evict_lowest_fee(&mut self) {
        let mut txs: Vec<_> = std::mem::take(&mut self.pending).into_vec();
        txs.sort_by(|a, b| b.cmp(a));
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

    /// Number of queued (future nonce) transactions.
    pub fn queued_len(&self) -> usize {
        self.queued.values().map(|q| q.len()).sum()
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_primitives::Keypair;
    use aether_types::{PublicKey, Signature};

    fn create_test_tx_with_keypair(kp: &Keypair, nonce: u64, fee: u128) -> Transaction {
        let sender_pubkey = PublicKey::from_bytes(kp.public_key().to_vec());
        let sender = sender_pubkey.to_address();
        let mut tx = Transaction {
            nonce,
            chain_id: 1,
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
            signature: Signature::from_bytes(vec![]),
        };

        let hash = tx.hash();
        let signature = kp.sign(hash.as_bytes());
        tx.signature = Signature::from_bytes(signature);
        tx
    }

    fn create_test_tx(nonce: u64, fee: u128) -> Transaction {
        let kp = Keypair::generate();
        create_test_tx_with_keypair(&kp, nonce, fee)
    }

    #[test]
    fn test_add_transaction() {
        let mut mempool = Mempool::with_defaults();
        let tx = create_test_tx(0, 60_000);
        mempool.add_transaction(tx).unwrap();
        assert_eq!(mempool.len(), 1);
    }

    #[test]
    fn test_priority_ordering() {
        let mut mempool = Mempool::with_defaults();

        // All nonce 0 from different senders — all go to pending
        let tx1 = create_test_tx(0, 110_000);
        let tx2 = create_test_tx(0, 160_000);
        let tx3 = create_test_tx(0, 130_000);

        mempool.add_transaction(tx1).unwrap();
        mempool.add_transaction(tx2).unwrap();
        mempool.add_transaction(tx3).unwrap();

        let txs = mempool.get_transactions(10, 1_000_000);
        assert_eq!(txs[0].fee, 160_000);
        assert_eq!(txs[1].fee, 130_000);
        assert_eq!(txs[2].fee, 110_000);
    }

    #[test]
    fn test_gas_limit() {
        let mut mempool = Mempool::with_defaults();
        let tx1 = create_test_tx(0, 90_000);
        let tx2 = create_test_tx(1, 120_000);

        mempool.add_transaction(tx1).unwrap();
        mempool.add_transaction(tx2).unwrap();

        let txs = mempool.get_transactions(10, 25000);
        assert_eq!(txs.len(), 1);
    }

    #[test]
    fn test_remove_transactions() {
        let mut mempool = Mempool::with_defaults();
        let tx1 = create_test_tx(0, 90_000);
        let tx2 = create_test_tx(1, 120_000);

        mempool.add_transaction(tx1.clone()).unwrap();
        mempool.add_transaction(tx2).unwrap();

        mempool.remove_transactions(&[tx1.hash()]);
        assert_eq!(mempool.len(), 1);
    }

    #[test]
    fn test_nonce_ordering_sequential() {
        let kp = Keypair::generate();
        let mut mempool = Mempool::with_defaults();

        // Nonces 0, 1, 2 in order — all go to pending
        let tx0 = create_test_tx_with_keypair(&kp, 0, 60_000);
        let tx1 = create_test_tx_with_keypair(&kp, 1, 60_000);
        let tx2 = create_test_tx_with_keypair(&kp, 2, 60_000);

        mempool.add_transaction(tx0).unwrap();
        mempool.add_transaction(tx1).unwrap();
        mempool.add_transaction(tx2).unwrap();

        assert_eq!(mempool.len(), 3);
        assert_eq!(mempool.queued_len(), 0);
    }

    #[test]
    fn test_nonce_gap_queues_future() {
        let kp = Keypair::generate();
        let mut mempool = Mempool::with_defaults();

        // Submit nonce 0, then skip to nonce 5
        let tx0 = create_test_tx_with_keypair(&kp, 0, 60_000);
        let tx5 = create_test_tx_with_keypair(&kp, 5, 60_000);

        mempool.add_transaction(tx0).unwrap();
        mempool.add_transaction(tx5).unwrap();

        assert_eq!(mempool.len(), 2); // Both tracked
        assert_eq!(mempool.queued_len(), 1); // nonce 5 is queued
    }

    #[test]
    fn test_nonce_too_low_rejected() {
        let kp = Keypair::generate();
        let mut mempool = Mempool::with_defaults();

        let sender_pubkey = PublicKey::from_bytes(kp.public_key().to_vec());
        let sender = sender_pubkey.to_address();
        mempool.set_sender_nonce(sender, 5);

        // Nonce 3 is below expected 5 — rejected
        let tx = create_test_tx_with_keypair(&kp, 3, 60_000);
        let result = mempool.add_transaction(tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonce too low"));
    }

    #[test]
    fn test_queued_promotion() {
        let kp = Keypair::generate();
        let mut mempool = Mempool::with_defaults();

        // Submit nonces out of order: 0, 3, 1, 2
        let tx0 = create_test_tx_with_keypair(&kp, 0, 60_000);
        let tx3 = create_test_tx_with_keypair(&kp, 3, 60_000);
        let tx1 = create_test_tx_with_keypair(&kp, 1, 60_000);
        let tx2 = create_test_tx_with_keypair(&kp, 2, 60_000);

        mempool.add_transaction(tx0).unwrap();
        assert_eq!(mempool.queued_len(), 0);

        mempool.add_transaction(tx3).unwrap();
        assert_eq!(mempool.queued_len(), 1); // nonce 3 queued

        mempool.add_transaction(tx1).unwrap();
        assert_eq!(mempool.queued_len(), 1); // nonce 3 still queued

        mempool.add_transaction(tx2).unwrap();
        // nonce 2 fills the gap → nonce 3 should be promoted
        assert_eq!(mempool.queued_len(), 0, "all txs should be promoted");
        assert_eq!(mempool.len(), 4);
    }

    #[test]
    fn test_reorg_readds_reverted_txs() {
        let kp = Keypair::generate();
        let mut mempool = Mempool::with_defaults();

        let tx0 = create_test_tx_with_keypair(&kp, 0, 60_000);
        let tx1 = create_test_tx_with_keypair(&kp, 1, 60_000);

        let sender = tx0.sender;

        // Verify the txs are valid before reorg
        assert!(tx0.verify_signature().is_ok(), "tx0 sig should be valid");
        assert!(tx1.verify_signature().is_ok(), "tx1 sig should be valid");

        // Reorg: block reverted, txs come back
        let mut new_nonces = HashMap::new();
        new_nonces.insert(sender, 0);

        // Test add_transaction directly first
        let mut mempool2 = Mempool::with_defaults();
        let r = mempool2.add_transaction(tx0.clone());
        assert!(r.is_ok(), "tx0 add failed: {:?}", r.err());
        let r = mempool2.add_transaction(tx1.clone());
        assert!(r.is_ok(), "tx1 add failed: {:?}", r.err());
        assert_eq!(mempool2.len(), 2);

        // Now test via reorg
        mempool.reorg(vec![tx0, tx1], new_nonces);
        assert_eq!(mempool.len(), 2, "reverted txs should be back in pool");
    }

    #[test]
    fn test_rate_limiting() {
        let kp = Keypair::generate();
        let mut mempool = Mempool::with_defaults();

        // Submit 100 txs — should succeed
        for i in 0..100u64 {
            let tx = create_test_tx_with_keypair(&kp, i, 60_000);
            mempool.add_transaction(tx).unwrap();
        }

        // 101st should be rate limited
        let tx = create_test_tx_with_keypair(&kp, 100, 60_000);
        let result = mempool.add_transaction(tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rate limited"));
    }
}
