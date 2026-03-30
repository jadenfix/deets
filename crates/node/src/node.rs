use aether_consensus::ConsensusEngine;
use aether_crypto_bls::BlsKeypair;
use aether_crypto_primitives::Keypair;
use aether_ledger::Ledger;
use aether_mempool::Mempool;
use aether_state_storage::Storage;
use aether_types::{
    Account, Address, Block, PublicKey, Slot, Transaction, TransactionReceipt, H256,
};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::time;

use crate::poh::{PohMetrics, PohRecorder};

pub struct Node {
    ledger: Ledger,
    mempool: Mempool,
    consensus: Box<dyn ConsensusEngine>,
    validator_key: Option<Keypair>,
    bls_key: Option<BlsKeypair>,
    running: bool,
    poh: PohRecorder,
    last_poh_metrics: Option<PohMetrics>,
    latest_block_hash: H256,
    latest_block_slot: Option<Slot>,
    blocks_by_slot: HashMap<Slot, H256>,
    blocks_by_hash: HashMap<H256, Block>,
    receipts: HashMap<H256, TransactionReceipt>,
}

impl Node {
    pub fn new<P: AsRef<Path>>(
        db_path: P,
        consensus: Box<dyn ConsensusEngine>,
        validator_key: Option<Keypair>,
        bls_key: Option<BlsKeypair>,
    ) -> Result<Self> {
        let storage = Storage::open(db_path).context("failed to open storage")?;
        let ledger = Ledger::new(storage).context("failed to initialize ledger")?;
        let mempool = Mempool::new();

        Ok(Node {
            ledger,
            mempool,
            consensus,
            validator_key,
            bls_key,
            running: false,
            poh: PohRecorder::new(),
            last_poh_metrics: None,
            latest_block_hash: H256::zero(),
            latest_block_slot: None,
            blocks_by_slot: HashMap::new(),
            blocks_by_hash: HashMap::new(),
            receipts: HashMap::new(),
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
            self.tick()?;

            // Wait for slot duration (500ms)
            time::sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }

    pub fn tick(&mut self) -> Result<()> {
        self.process_slot()?;
        self.consensus.advance_slot();
        Ok(())
    }

    fn process_slot(&mut self) -> Result<()> {
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
        let mut receipts = self.ledger.apply_block_transactions(&transactions)?;
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
            self.latest_block_hash,
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

        // Create vote for our own block (BLS signature)
        let validator_pubkey =
            PublicKey::from_bytes(self.validator_key.as_ref().unwrap().public_key());

        // Sign vote with BLS keypair for valid aggregation
        let vote_msg = {
            let mut msg = Vec::new();
            msg.extend_from_slice(block_hash.as_bytes());
            msg.extend_from_slice(&slot.to_le_bytes());
            msg
        };
        let vote_sig = if let Some(bls) = &self.bls_key {
            bls.sign(&vote_msg)
        } else {
            vec![0u8; 96]
        };

        match self.consensus.add_vote(aether_types::Vote {
            slot,
            block_hash,
            validator: validator_pubkey,
            signature: aether_types::Signature::from_bytes(vote_sig),
            stake: self.consensus.total_stake(), // Single validator gets all stake
        }) {
            Ok(()) => println!("  Vote created and processed"),
            Err(e) => println!("  Vote failed: {e}"),
        }

        for receipt in &mut receipts {
            receipt.block_hash = block_hash;
            receipt.slot = slot;
            self.receipts.insert(receipt.tx_hash, receipt.clone());
        }

        self.latest_block_hash = block_hash;
        self.latest_block_slot = Some(slot);
        self.blocks_by_slot.insert(slot, block_hash);
        self.blocks_by_hash.insert(block_hash, block);

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

    pub fn get_state_root(&self) -> H256 {
        self.ledger.state_root()
    }

    pub fn mempool_size(&self) -> usize {
        self.mempool.len()
    }

    pub fn poh_metrics(&self) -> Option<&PohMetrics> {
        self.last_poh_metrics.as_ref()
    }

    pub fn current_slot(&self) -> Slot {
        self.consensus.current_slot()
    }

    pub fn finalized_slot(&self) -> Slot {
        self.consensus.finalized_slot()
    }

    pub fn latest_block_slot(&self) -> Option<Slot> {
        self.latest_block_slot
    }

    pub fn seed_account(&mut self, address: &Address, balance: u128) -> Result<()> {
        self.ledger.seed_account(address, balance)
    }

    pub fn get_block_by_slot(&self, slot: Slot) -> Option<Block> {
        self.blocks_by_slot
            .get(&slot)
            .and_then(|hash| self.blocks_by_hash.get(hash))
            .cloned()
    }

    pub fn get_block_by_hash(&self, hash: H256) -> Option<Block> {
        self.blocks_by_hash.get(&hash).cloned()
    }

    pub fn get_transaction_receipt(&self, tx_hash: H256) -> Option<TransactionReceipt> {
        self.receipts.get(&tx_hash).cloned()
    }

    pub fn get_account(&self, address: Address) -> Result<Option<Account>> {
        self.ledger.get_account(&address)
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

    #[test]
    fn updates_poh_metrics_each_slot() {
        let temp_dir = TempDir::new().unwrap();
        let keypair = Keypair::generate();
        let validators = vec![validator_info_from_key(&keypair)];
        let consensus = Box::new(SimpleConsensus::new(validators));

        let mut node = Node::new(temp_dir.path(), consensus, Some(keypair), None).unwrap();

        node.process_slot().unwrap();
        let first_metrics = node.poh_metrics().cloned().unwrap();
        assert_eq!(first_metrics.tick_count, 1);

        node.process_slot().unwrap();
        let second_metrics = node.poh_metrics().cloned().unwrap();
        assert!(second_metrics.tick_count >= 2);
        assert!(second_metrics.average_duration_ms >= 0.0);
    }
}
