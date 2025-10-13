use aether_consensus::SimpleConsensus;
use aether_crypto_primitives::Keypair;
use aether_ledger::Ledger;
use aether_mempool::Mempool;
use aether_state_storage::Storage;
use aether_types::{Block, PublicKey, Slot, Transaction, ValidatorInfo, VrfProof, H256};
use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::time;

use crate::poh::{PohMetrics, PohRecorder};

pub struct Node {
    ledger: Ledger,
    mempool: Mempool,
    consensus: SimpleConsensus,
    validator_key: Option<Keypair>,
    running: bool,
    poh: PohRecorder,
    last_poh_metrics: Option<PohMetrics>,
}

impl Node {
    pub fn new<P: AsRef<Path>>(
        db_path: P,
        validators: Vec<ValidatorInfo>,
        validator_key: Option<Keypair>,
    ) -> Result<Self> {
        let storage = Storage::open(db_path).context("failed to open storage")?;
        let ledger = Ledger::new(storage).context("failed to initialize ledger")?;
        let mempool = Mempool::new();
        let consensus = SimpleConsensus::new(validators);

        Ok(Node {
            ledger,
            mempool,
            consensus,
            validator_key,
            running: false,
            poh: PohRecorder::new(),
            last_poh_metrics: None,
        })
    }

    pub fn submit_transaction(&mut self, tx: Transaction) -> Result<H256> {
        let tx_hash = tx.hash();
        self.mempool.add_transaction(tx)?;
        Ok(tx_hash)
    }

    pub async fn run(&mut self) -> Result<()> {
        self.running = true;

        println!("Node starting...");
        println!("Validator: {}", self.validator_key.is_some());
        println!("Starting slot: {}", self.consensus.current_slot());

        while self.running {
            self.process_slot().await?;

            // Wait for slot duration (500ms)
            time::sleep(Duration::from_millis(500)).await;

            self.consensus.advance_slot();
        }

        Ok(())
    }

    async fn process_slot(&mut self) -> Result<()> {
        let slot = self.consensus.current_slot();

        let metrics = self.poh.tick(Instant::now());
        self.last_poh_metrics = Some(metrics.clone());
        println!(
            "PoH tick {} ms avg {:.1} jitter {:.1}",
            metrics.last_duration_ms, metrics.average_duration_ms, metrics.jitter_ms
        );

        if let Some(ref keypair) = self.validator_key {
            let pubkey = PublicKey::from_bytes(keypair.public_key());

            if self.consensus.is_leader(slot, &pubkey) {
                println!("Slot {}: I am leader, producing block", slot);
                self.produce_block(slot)?;
            } else {
                println!("Slot {}: Not leader, waiting for block", slot);
            }
        }

        // Check if any slot can be finalized
        self.check_finality();

        Ok(())
    }

    fn produce_block(&mut self, slot: Slot) -> Result<()> {
        // Get transactions from mempool
        let transactions = self.mempool.get_transactions(1000, 5_000_000);

        if transactions.is_empty() {
            println!("  No transactions to include");
            return Ok(());
        }

        println!("  Including {} transactions", transactions.len());

        // Apply transactions to ledger
        let receipts = self.ledger.apply_block_transactions(&transactions)?;
        let successful = receipts
            .iter()
            .filter(|r| matches!(r.status, aether_types::TransactionStatus::Success))
            .count();

        println!(
            "  {} successful, {} failed",
            successful,
            receipts.len() - successful
        );

        // Create block
        let state_root = self.ledger.state_root();
        let proposer = self.validator_key.as_ref().unwrap().to_address();

        let _block = Block::new(
            slot,
            H256::zero(), // parent hash - would track in production
            aether_types::Address::from_slice(&proposer).unwrap(),
            VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            transactions.clone(),
        );

        // Remove transactions from mempool
        let tx_hashes: Vec<H256> = transactions.iter().map(|tx| tx.hash()).collect();
        self.mempool.remove_transactions(&tx_hashes);

        println!("  Block produced with state root: {}", state_root);

        Ok(())
    }

    fn check_finality(&mut self) {
        let current_slot = self.consensus.current_slot();

        // Check last few slots for finality
        for slot in self.consensus.finalized_slot()..current_slot {
            if self.consensus.check_finality(slot) {
                println!("Slot {} finalized!", slot);
            }
        }
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn get_state_root(&self) -> H256 {
        self.ledger.state_root()
    }

    pub fn mempool_size(&self) -> usize {
        self.mempool.len()
    }

    pub fn poh_metrics(&self) -> Option<&PohMetrics> {
        self.last_poh_metrics.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::{PublicKey, ValidatorInfo};
    use tempfile::TempDir;

    fn validator_info_from_key(keypair: &Keypair) -> ValidatorInfo {
        ValidatorInfo {
            pubkey: PublicKey::from_bytes(keypair.public_key()),
            stake: 1_000,
            commission: 0,
            active: true,
        }
    }

    #[tokio::test]
    async fn updates_poh_metrics_each_slot() {
        let temp_dir = TempDir::new().unwrap();
        let keypair = Keypair::generate();
        let validators = vec![validator_info_from_key(&keypair)];

        let mut node = Node::new(temp_dir.path(), validators, Some(keypair)).unwrap();

        node.process_slot().await.unwrap();
        let first_metrics = node.poh_metrics().cloned().unwrap();
        assert_eq!(first_metrics.tick_count, 1);

        node.process_slot().await.unwrap();
        let second_metrics = node.poh_metrics().cloned().unwrap();
        assert!(second_metrics.tick_count >= 2);
        assert!(second_metrics.average_duration_ms >= 0.0);
    }
}
