use aether_consensus::ConsensusEngine;
use aether_crypto_primitives::Keypair;
use aether_ledger::{ChainStore, Ledger};
use aether_mempool::Mempool;
use aether_state_storage::Storage;
use aether_types::{
    Block, PublicKey, Slot, Transaction, TransactionReceipt, TransactionStatus, H256,
};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
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
    chain_store: ChainStore,
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
        let chain_store = ChainStore::new(ledger.storage());

        Ok(Node {
            ledger,
            mempool,
            consensus,
            validator_key,
            running: false,
            poh: PohRecorder::new(),
            last_poh_metrics: None,
            chain_store,
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
        let mut receipts = self
            .ledger
            .apply_block_transactions(&transactions)
            .context("failed to execute transactions for block")?;
        let successful = receipts
            .iter()
            .filter(|r| matches!(r.status, TransactionStatus::Success))
            .count();

        if !transactions.is_empty() {
            println!(
                "  {} successful, {} failed",
                successful,
                receipts.len() - successful
            );
        }

        let tx_hashes: Vec<H256> = transactions.iter().map(|tx| tx.hash()).collect();
        let transactions_root = compute_transactions_root(&transactions);
        let receipts_root =
            compute_receipts_root(&receipts).context("failed to compute receipts root")?;
        let state_root = self.ledger.state_root();

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
        let proposer = self.validator_key.as_ref().unwrap().to_address();

        let mut block = Block::new(
            slot,
            H256::zero(), // parent hash - would track in production
            aether_types::Address::from_slice(&proposer).unwrap(),
            vrf_proof,
            transactions.clone(),
        );

        block.header.state_root = state_root;
        block.header.transactions_root = transactions_root;
        block.header.receipts_root = receipts_root;

        let block_hash = block.hash();
        println!("  Block produced: {:?}", block_hash);
        println!("  State root: {}", state_root);

        for receipt in receipts.iter_mut() {
            receipt.block_hash = block_hash;
            receipt.slot = slot;
            receipt.state_root = state_root;
        }

        // Validate our own block
        if let Err(e) = self.consensus.validate_block(&block) {
            println!("  WARNING: Block validation failed: {}", e);
            return Ok(());
        }

        self.chain_store
            .store_block(&block, &receipts)
            .context("failed to persist block and receipts")?;

        // Ask the consensus engine to produce a signed vote for this block.
        if let Some(mut vote) = self.consensus.create_vote(block_hash)? {
            // Defensive: ensure vote references the block we just produced.
            vote.slot = slot;
            vote.block_hash = block_hash;
            self.consensus.add_vote(vote)?;
            println!("  Vote created with consensus signing and processed");
        }

        // Remove transactions from mempool
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

fn compute_transactions_root(transactions: &[Transaction]) -> H256 {
    let hashes: Vec<H256> = transactions.iter().map(|tx| tx.hash()).collect();
    merkle_root(hashes)
}

fn compute_receipts_root(receipts: &[TransactionReceipt]) -> Result<H256> {
    let mut leaves = Vec::with_capacity(receipts.len());

    for receipt in receipts {
        let mut normalized = receipt.clone();
        normalized.block_hash = H256::zero();

        let encoded = bincode::serialize(&normalized)
            .context("failed to serialize receipt during receipts root computation")?;
        leaves.push(hash_data(&encoded));
    }

    Ok(merkle_root(leaves))
}

fn hash_data(bytes: &[u8]) -> H256 {
    let digest = Sha256::digest(bytes);
    let mut arr = [0u8; 32];
    arr.copy_from_slice(digest.as_slice());
    H256(arr)
}

fn merkle_root(mut leaves: Vec<H256>) -> H256 {
    if leaves.is_empty() {
        return H256::zero();
    }

    while leaves.len() > 1 {
        let mut next = Vec::with_capacity((leaves.len() + 1) / 2);
        for pair in leaves.chunks(2) {
            let mut combined = Vec::with_capacity(64);
            combined.extend_from_slice(pair[0].as_bytes());
            if pair.len() == 2 {
                combined.extend_from_slice(pair[1].as_bytes());
            } else {
                combined.extend_from_slice(pair[0].as_bytes());
            }
            next.push(hash_data(&combined));
        }
        leaves = next;
    }

    leaves.pop().unwrap()
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
