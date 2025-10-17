use aether_types::Transaction;
use anyhow::Result;
use std::collections::HashSet;

/// Parallel Scheduler for Transaction Execution
///
/// Uses declared R/W sets to partition transactions into non-conflicting
/// batches that can be executed in parallel.
///
/// Algorithm:
/// 1. Build conflict graph from R/W sets
/// 2. Color graph (greedy coloring)
/// 3. Each color = independent batch
/// 4. Execute batches in parallel using rayon
///
/// Conflict Rule (from spec):
/// tx_a conflicts with tx_b if:
/// - W(a) ∩ W(b) ≠ ∅ (write-write)
/// - W(a) ∩ R(b) ≠ ∅ (write-read)
/// - W(b) ∩ R(a) ≠ ∅ (read-write)
/// - Inputs(a) ∩ Inputs(b) ≠ ∅ (UTxO conflict)
///
pub struct ParallelScheduler {
    /// Maximum batch size
    max_batch_size: usize,
}

impl ParallelScheduler {
    pub fn new() -> Self {
        ParallelScheduler {
            max_batch_size: 1000,
        }
    }

    /// Partition transactions into non-conflicting batches
    pub fn schedule(&self, transactions: &[Transaction]) -> Vec<Vec<Transaction>> {
        if transactions.is_empty() {
            return vec![];
        }

        let mut batches: Vec<Vec<Transaction>> = vec![];
        let mut remaining: Vec<Transaction> = transactions.to_vec();

        while !remaining.is_empty() {
            let mut current_batch = vec![];
            let mut used_indices = HashSet::new();

            // Greedily build a non-conflicting batch
            for (i, tx) in remaining.iter().enumerate() {
                if used_indices.contains(&i) {
                    continue;
                }

                // Check if tx conflicts with any in current batch
                let mut conflicts = false;
                for batch_tx in &current_batch {
                    if tx.conflicts_with(batch_tx) {
                        conflicts = true;
                        break;
                    }
                }

                if !conflicts && !Self::has_pending_dependencies(tx, i, &remaining, &used_indices) {
                    current_batch.push(tx.clone());
                    used_indices.insert(i);

                    if current_batch.len() >= self.max_batch_size {
                        break;
                    }
                }
            }

            // Remove used transactions from remaining
            remaining = remaining
                .into_iter()
                .enumerate()
                .filter(|(i, _)| !used_indices.contains(i))
                .map(|(_, tx)| tx)
                .collect();

            if !current_batch.is_empty() {
                batches.push(current_batch);
            } else {
                // Safety: if we can't make progress, stop
                break;
            }
        }

        batches
    }

    fn has_pending_dependencies(
        tx: &Transaction,
        idx: usize,
        remaining: &[Transaction],
        used_indices: &HashSet<usize>,
    ) -> bool {
        for addr in &tx.reads {
            for (j, other) in remaining.iter().enumerate() {
                if j == idx || used_indices.contains(&j) {
                    continue;
                }

                if other.writes.contains(addr) {
                    return true;
                }
            }
        }
        false
    }

    /// Execute batches in parallel using Rayon
    /// Each batch is executed sequentially (they have dependencies),
    /// but within each batch transactions execute in parallel
    pub fn execute_parallel<F>(&self, batches: Vec<Vec<Transaction>>, executor: F) -> Result<()>
    where
        F: Fn(&Transaction) -> Result<()> + Sync + Send,
    {
        use rayon::prelude::*;

        // Execute each batch sequentially (batches have dependencies)
        // Within each batch, transactions run in parallel
        for batch in batches {
            // Parallel execution within batch using rayon
            batch.par_iter().try_for_each(|tx| executor(tx))?;
        }

        Ok(())
    }

    /// Calculate potential speedup
    pub fn speedup_estimate(&self, transactions: &[Transaction]) -> f64 {
        if transactions.is_empty() {
            return 1.0;
        }

        let batches = self.schedule(transactions);

        // Speedup = total_txs / num_sequential_steps
        // where num_sequential_steps = number of batches
        transactions.len() as f64 / batches.len() as f64
    }
}

impl Default for ParallelScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::{Address, PublicKey, Signature};
    use std::collections::HashSet;

    fn create_test_tx(reads: Vec<u8>, writes: Vec<u8>) -> Transaction {
        let read_addrs: HashSet<Address> = reads
            .iter()
            .map(|&b| Address::from_slice(&[b; 20]).unwrap())
            .collect();

        let write_addrs: HashSet<Address> = writes
            .iter()
            .map(|&b| Address::from_slice(&[b; 20]).unwrap())
            .collect();

        Transaction {
            nonce: 0,
            sender: Address::from_slice(&[1u8; 20]).unwrap(),
            sender_pubkey: PublicKey::from_bytes(vec![2u8; 32]),
            inputs: vec![],
            outputs: vec![],
            reads: read_addrs,
            writes: write_addrs,
            program_id: None,
            data: vec![],
            gas_limit: 21000,
            fee: 1000,
            signature: Signature::from_bytes(vec![0u8; 64]),
        }
    }

    #[test]
    fn test_non_conflicting_transactions() {
        let scheduler = ParallelScheduler::new();

        // Three transactions with disjoint R/W sets
        let tx1 = create_test_tx(vec![], vec![1]);
        let tx2 = create_test_tx(vec![], vec![2]);
        let tx3 = create_test_tx(vec![], vec![3]);

        let batches = scheduler.schedule(&[tx1, tx2, tx3]);

        // Should all be in one batch (no conflicts)
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 3);
    }

    #[test]
    fn test_conflicting_transactions() {
        let scheduler = ParallelScheduler::new();

        // Two transactions writing to same address
        let tx1 = create_test_tx(vec![], vec![1]);
        let tx2 = create_test_tx(vec![], vec![1]); // Conflicts with tx1

        let batches = scheduler.schedule(&[tx1, tx2]);

        // Should be in separate batches
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 1);
        assert_eq!(batches[1].len(), 1);
    }

    #[test]
    fn test_read_write_conflict() {
        let scheduler = ParallelScheduler::new();

        // tx1 writes, tx2 reads same address
        let tx1 = create_test_tx(vec![], vec![1]);
        let tx2 = create_test_tx(vec![1], vec![]);

        let batches = scheduler.schedule(&[tx1, tx2]);

        // Should be in separate batches (write-read conflict)
        assert_eq!(batches.len(), 2);
    }

    #[test]
    fn test_complex_dependencies() {
        let scheduler = ParallelScheduler::new();

        // tx1: W(1)
        // tx2: W(2)
        // tx3: R(1), W(3)
        // tx4: R(2), W(4)
        // tx5: R(3, 4), W(5)

        let tx1 = create_test_tx(vec![], vec![1]);
        let tx2 = create_test_tx(vec![], vec![2]);
        let tx3 = create_test_tx(vec![1], vec![3]);
        let tx4 = create_test_tx(vec![2], vec![4]);
        let tx5 = create_test_tx(vec![3, 4], vec![5]);

        let batches = scheduler.schedule(&[tx1, tx2, tx3, tx4, tx5]);

        // Expected batches:
        // Batch 0: tx1, tx2 (no conflicts)
        // Batch 1: tx3, tx4 (depend on batch 0, but independent of each other)
        // Batch 2: tx5 (depends on batch 1)

        assert!(batches.len() >= 3);

        // First batch should have tx1 and tx2
        assert!(batches[0].len() >= 2);
    }

    #[test]
    fn test_speedup_estimate() {
        let scheduler = ParallelScheduler::new();

        // All independent transactions
        let txs: Vec<Transaction> = (0..10).map(|i| create_test_tx(vec![], vec![i])).collect();

        let speedup = scheduler.speedup_estimate(&txs);

        // Should be close to 10x (all can run in parallel)
        assert!(speedup > 5.0);
    }

    #[test]
    fn test_empty_schedule() {
        let scheduler = ParallelScheduler::new();
        let batches = scheduler.schedule(&[]);

        assert_eq!(batches.len(), 0);
    }
}
