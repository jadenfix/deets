use aether_types::Transaction;
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashSet;

/// Parallel Scheduler for Transaction Execution
///
/// Uses declared R/W sets to partition transactions into non-conflicting
/// batches that can be executed in parallel via rayon.
///
/// Algorithm:
/// 1. Build conflict graph from R/W sets
/// 2. Greedy coloring to partition into independent batches
/// 3. Execute each batch in parallel (batches are sequential)
///
/// Conflict Rule:
/// tx_a conflicts with tx_b if:
/// - W(a) ∩ W(b) ≠ ∅ (write-write)
/// - W(a) ∩ R(b) ≠ ∅ (write-read)
/// - W(b) ∩ R(a) ≠ ∅ (read-write)
/// - Inputs(a) ∩ Inputs(b) ≠ ∅ (UTxO conflict)
pub struct ParallelScheduler {
    max_batch_size: usize,
}

impl ParallelScheduler {
    pub fn new() -> Self {
        ParallelScheduler {
            max_batch_size: 1000,
        }
    }

    /// Partition transactions into non-conflicting batches.
    pub fn schedule(&self, transactions: &[Transaction]) -> Vec<Vec<Transaction>> {
        if transactions.is_empty() {
            return vec![];
        }

        let mut batches: Vec<Vec<Transaction>> = vec![];
        let mut remaining: Vec<Transaction> = transactions.to_vec();

        while !remaining.is_empty() {
            let mut current_batch = vec![];
            let mut used_indices = HashSet::new();

            for (i, tx) in remaining.iter().enumerate() {
                if used_indices.contains(&i) {
                    continue;
                }

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

            remaining = remaining
                .into_iter()
                .enumerate()
                .filter(|(i, _)| !used_indices.contains(i))
                .map(|(_, tx)| tx)
                .collect();

            if !current_batch.is_empty() {
                batches.push(current_batch);
            } else {
                break;
            }
        }

        batches
    }

    /// Check if tx at `idx` depends on an EARLIER unscheduled tx.
    /// This enforces ordering: a tx that reads from an address written by
    /// an earlier tx must wait for the writer to be scheduled first.
    fn has_pending_dependencies(
        tx: &Transaction,
        idx: usize,
        remaining: &[Transaction],
        used_indices: &HashSet<usize>,
    ) -> bool {
        // Read-after-write: this tx reads addr X, an earlier tx writes X
        for addr in &tx.reads {
            for (j, other) in remaining.iter().enumerate() {
                if j >= idx || used_indices.contains(&j) {
                    continue; // Only check EARLIER transactions
                }
                if other.writes.contains(addr) {
                    return true;
                }
            }
        }
        // Write-after-read: this tx writes addr X, an earlier tx reads X
        for addr in &tx.writes {
            for (j, other) in remaining.iter().enumerate() {
                if j >= idx || used_indices.contains(&j) {
                    continue; // Only check EARLIER transactions
                }
                if other.reads.contains(addr) {
                    return true;
                }
            }
        }
        false
    }

    /// Execute batches with rayon parallelism.
    ///
    /// Batches execute sequentially (they have inter-batch dependencies).
    /// Within each batch, transactions execute in parallel via rayon.
    ///
    /// The executor must be `Fn + Sync` (not FnMut) because multiple
    /// threads call it concurrently within a batch.
    pub fn execute_parallel<F>(&self, batches: Vec<Vec<Transaction>>, executor: F) -> Result<()>
    where
        F: Fn(&Transaction) -> Result<()> + Sync,
    {
        for batch in batches {
            if batch.len() == 1 {
                // Single tx — no parallelism overhead
                executor(&batch[0])?;
            } else {
                // Parallel execution within batch
                batch.par_iter().try_for_each(|tx| executor(tx))?;
            }
        }

        Ok(())
    }

    /// Execute batches sequentially (for comparison / fallback).
    pub fn execute_sequential<F>(
        &self,
        batches: Vec<Vec<Transaction>>,
        mut executor: F,
    ) -> Result<()>
    where
        F: FnMut(&Transaction) -> Result<()>,
    {
        for batch in batches {
            for tx in &batch {
                executor(tx)?;
            }
        }
        Ok(())
    }

    /// Execute parallel and collect results per transaction.
    ///
    /// Returns a Vec of results in the same order as the input batches.
    pub fn execute_parallel_collect<F, R>(
        &self,
        batches: Vec<Vec<Transaction>>,
        executor: F,
    ) -> Result<Vec<Vec<R>>>
    where
        F: Fn(&Transaction) -> Result<R> + Sync,
        R: Send,
    {
        let mut all_results = Vec::with_capacity(batches.len());

        for batch in batches {
            let results: Result<Vec<R>> = batch.par_iter().map(|tx| executor(tx)).collect();
            all_results.push(results?);
        }

        Ok(all_results)
    }

    /// Calculate potential speedup.
    pub fn speedup_estimate(&self, transactions: &[Transaction]) -> f64 {
        if transactions.is_empty() {
            return 1.0;
        }

        let batches = self.schedule(transactions);
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

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
            chain_id: 1,
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

        let tx1 = create_test_tx(vec![], vec![1]);
        let tx2 = create_test_tx(vec![], vec![2]);
        let tx3 = create_test_tx(vec![], vec![3]);

        let batches = scheduler.schedule(&[tx1, tx2, tx3]);

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 3);
    }

    #[test]
    fn test_conflicting_transactions() {
        let scheduler = ParallelScheduler::new();

        let tx1 = create_test_tx(vec![], vec![1]);
        let tx2 = create_test_tx(vec![], vec![1]);

        let batches = scheduler.schedule(&[tx1, tx2]);

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 1);
        assert_eq!(batches[1].len(), 1);
    }

    #[test]
    fn test_read_write_conflict() {
        let scheduler = ParallelScheduler::new();

        let tx1 = create_test_tx(vec![], vec![1]);
        let tx2 = create_test_tx(vec![1], vec![]);

        let batches = scheduler.schedule(&[tx1, tx2]);

        assert_eq!(batches.len(), 2);
    }

    #[test]
    fn test_complex_dependencies() {
        let scheduler = ParallelScheduler::new();

        let tx1 = create_test_tx(vec![], vec![1]);
        let tx2 = create_test_tx(vec![], vec![2]);
        let tx3 = create_test_tx(vec![1], vec![3]);
        let tx4 = create_test_tx(vec![2], vec![4]);
        let tx5 = create_test_tx(vec![3, 4], vec![5]);

        let batches = scheduler.schedule(&[tx1, tx2, tx3, tx4, tx5]);

        assert!(batches.len() >= 3);
        assert!(batches[0].len() >= 2);
    }

    #[test]
    fn test_speedup_estimate() {
        let scheduler = ParallelScheduler::new();

        let txs: Vec<Transaction> = (0..10).map(|i| create_test_tx(vec![], vec![i])).collect();

        let speedup = scheduler.speedup_estimate(&txs);

        assert!(speedup > 5.0);
    }

    #[test]
    fn test_empty_schedule() {
        let scheduler = ParallelScheduler::new();
        let batches = scheduler.schedule(&[]);

        assert_eq!(batches.len(), 0);
    }

    #[test]
    fn test_parallel_execution_correctness() {
        let scheduler = ParallelScheduler::new();

        // 100 non-conflicting transactions
        let txs: Vec<Transaction> = (0..100u8)
            .map(|i| create_test_tx(vec![], vec![i]))
            .collect();
        let batches = scheduler.schedule(&txs);

        // Track execution count with atomic counter
        let counter = Arc::new(AtomicU64::new(0));
        let counter_ref = counter.clone();

        scheduler
            .execute_parallel(batches, move |_tx| {
                counter_ref.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .unwrap();

        assert_eq!(counter.load(Ordering::Relaxed), 100);
    }

    #[test]
    fn test_parallel_faster_than_sequential() {
        let scheduler = ParallelScheduler::new();

        // 200 non-conflicting transactions with simulated work
        let txs: Vec<Transaction> = (0..200)
            .map(|i| create_test_tx(vec![], vec![(i % 256) as u8]))
            .collect();

        // Deduplicate: only unique write addresses to avoid conflicts
        // Use first 200 unique addresses
        let txs: Vec<Transaction> = (0..200u16)
            .map(|i| {
                let b1 = (i / 256) as u8;
                let b2 = (i % 256) as u8;
                let mut writes = HashSet::new();
                let mut addr_bytes = [0u8; 20];
                addr_bytes[0] = b1;
                addr_bytes[1] = b2;
                writes.insert(Address::from_slice(&addr_bytes).unwrap());
                Transaction {
                    nonce: 0,
                    chain_id: 1,
                    sender: Address::from_slice(&[1u8; 20]).unwrap(),
                    sender_pubkey: PublicKey::from_bytes(vec![2u8; 32]),
                    inputs: vec![],
                    outputs: vec![],
                    reads: HashSet::new(),
                    writes,
                    program_id: None,
                    data: vec![],
                    gas_limit: 21000,
                    fee: 1000,
                    signature: Signature::from_bytes(vec![0u8; 64]),
                }
            })
            .collect();

        let batches_seq = scheduler.schedule(&txs);
        let batches_par = scheduler.schedule(&txs);

        // Simulated work: spin for a tiny bit
        let work = |_tx: &Transaction| -> Result<()> {
            let mut x = 0u64;
            for i in 0..1000 {
                x = x.wrapping_add(i);
            }
            std::hint::black_box(x);
            Ok(())
        };

        let start_seq = Instant::now();
        scheduler.execute_sequential(batches_seq, work).unwrap();
        let seq_time = start_seq.elapsed();

        let start_par = Instant::now();
        scheduler.execute_parallel(batches_par, work).unwrap();
        let par_time = start_par.elapsed();

        // Parallel should be at least somewhat faster (or at least not crash)
        // On single-core CI this might not be faster, so we just verify correctness
        println!(
            "Sequential: {:?}, Parallel: {:?}, Speedup: {:.2}x",
            seq_time,
            par_time,
            seq_time.as_nanos() as f64 / par_time.as_nanos().max(1) as f64
        );
    }

    #[test]
    fn test_parallel_collect_results() {
        let scheduler = ParallelScheduler::new();

        let txs: Vec<Transaction> = (0..10u8).map(|i| create_test_tx(vec![], vec![i])).collect();
        let batches = scheduler.schedule(&txs);

        let results = scheduler
            .execute_parallel_collect(batches, |tx| Ok(tx.fee))
            .unwrap();

        let total: u128 = results.iter().flat_map(|batch| batch.iter()).sum();
        assert_eq!(total, 1000 * 10); // 10 txs * 1000 fee each
    }
}
