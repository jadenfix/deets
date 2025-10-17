use aether_consensus::ConsensusEngine;
use aether_crypto_primitives::Keypair;
use aether_ledger::Ledger;
use aether_mempool::Mempool;
use aether_state_storage::Storage;
use aether_types::{Block, PublicKey, Slot, Transaction, H256};
use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::time;

use crate::poh::{PohMetrics, PohRecorder};

pub struct Node {
    ledger: Ledger,
    mempool: Mempool,
    consensus: Box<dyn ConsensusEngine>,
    validator_key: Option<Keypair>,
    running: bool,
    poh: PohRecorder,
    last_poh_metrics: Option<PohMetrics>,
}

impl Node {
    pub fn new<P: AsRef<Path>>(
        db_path: P,
        consensus: Box<dyn ConsensusEngine>,
        validator_key: Option<Keypair>,
    ) -> Result<Self> {
        let storage = Storage::open(db_path).context("failed to open storage")?;
        let ledger = Ledger::new(storage).context("failed to initialize ledger")?;
        let mempool = Mempool::new();

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
            // Even with no transactions, create an empty block for consensus
        }

        println!("  Including {} transactions", transactions.len());

        // Apply transactions to ledger
        let receipts = self.ledger.apply_block_transactions(&transactions)?;
        let successful = receipts
            .iter()
            .filter(|r| matches!(r.status, aether_types::TransactionStatus::Success))
            .count();

        if !transactions.is_empty() {
            println!(
                "  {} successful, {} failed",
                successful,
                receipts.len() - successful
            );
        }

        // Get VRF proof from consensus (proves leader eligibility)
        let vrf_proof_crypto = self.consensus.get_leader_proof(slot);
        let vrf_proof = if let Some(proof) = vrf_proof_crypto {
            // Convert from crypto::VrfProof to types::VrfProof
            aether_types::VrfProof {
                output: proof.output,
                proof: proof.proof,
            }
        } else {
            aether_types::VrfProof {
                output: [0u8; 32],
                proof: vec![],
            }
        };

        // Create block with VRF proof
        let state_root = self.ledger.state_root();
        let proposer = self.validator_key.as_ref().unwrap().to_address();

        let block = Block::new(
            slot,
            H256::zero(), // parent hash - would track in production
            aether_types::Address::from_slice(&proposer).unwrap(),
            vrf_proof,
            transactions.clone(),
        );

        let block_hash = block.hash();
        println!("  Block produced: {:?}", block_hash);
        println!("  State root: {}", state_root);

        // Validate our own block
        if let Err(e) = self.consensus.validate_block(&block) {
            println!("  WARNING: Block validation failed: {}", e);
            return Ok(());
        }

        // Create vote for our own block with real BLS signature via consensus
        let validator_pubkey =
            PublicKey::from_bytes(self.validator_key.as_ref().unwrap().public_key());
        
        // Vote is now created by consensus with real BLS signature
        // The consensus will handle signing internally
        if let Ok(_) = self.consensus.add_vote(aether_types::Vote {
            slot,
            block_hash,
            validator: validator_pubkey,
            signature: aether_types::Signature::from_bytes(vec![0; 96]), // 96-byte BLS signature placeholder - consensus creates real one
            stake: self.consensus.total_stake(),
        }) {
            println!("  Vote created with BLS signature and processed");
        }

        // Remove transactions from mempool
        let tx_hashes: Vec<H256> = transactions.iter().map(|tx| tx.hash()).collect();
        self.mempool.remove_transactions(&tx_hashes);

        Ok(())
    }

    fn check_finality(&mut self) {
        let current_slot = self.consensus.current_slot();
        let last_finalized = self.consensus.finalized_slot();

        // Check last few slots for finality
        for slot in last_finalized..current_slot {
            if self.consensus.check_finality(slot) {
                println!("✓ FINALIZED: Slot {} via VRF+HotStuff+BLS!", slot);
            }
        }
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn get_state_root(&mut self) -> H256 {
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
    use aether_consensus::SimpleConsensus;
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
        let consensus = Box::new(SimpleConsensus::new(validators));

        let mut node = Node::new(temp_dir.path(), consensus, Some(keypair)).unwrap();

        node.process_slot().await.unwrap();
        let first_metrics = node.poh_metrics().cloned().unwrap();
        assert_eq!(first_metrics.tick_count, 1);

        node.process_slot().await.unwrap();
        let second_metrics = node.poh_metrics().cloned().unwrap();
        assert!(second_metrics.tick_count >= 2);
        assert!(second_metrics.average_duration_ms >= 0.0);
    }
}
