use crate::RpcBackend;
use aether_consensus::ConsensusEngine;
use aether_ledger::{ChainStore, Ledger};
use aether_mempool::Mempool;
use aether_types::{Account, Address, Block, Transaction, TransactionReceipt, H256};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub struct NodeRpcBackend {
    ledger: Arc<RwLock<Ledger>>,
    mempool: Arc<RwLock<Mempool>>,
    consensus: Arc<RwLock<Box<dyn ConsensusEngine>>>,
    chain_store: Arc<ChainStore>,
}

impl NodeRpcBackend {
    pub fn new(
        ledger: Arc<RwLock<Ledger>>,
        mempool: Arc<RwLock<Mempool>>,
        consensus: Arc<RwLock<Box<dyn ConsensusEngine>>>,
        chain_store: Arc<ChainStore>,
    ) -> Self {
        Self {
            ledger,
            mempool,
            consensus,
            chain_store,
        }
    }

    fn ledger_read(&self) -> Result<RwLockReadGuard<'_, Ledger>> {
        self.ledger
            .read()
            .map_err(|_| anyhow!("ledger lock poisoned"))
    }

    fn ledger_write(&self) -> Result<RwLockWriteGuard<'_, Ledger>> {
        self.ledger
            .write()
            .map_err(|_| anyhow!("ledger lock poisoned"))
    }

    fn mempool_write(&self) -> Result<RwLockWriteGuard<'_, Mempool>> {
        self.mempool
            .write()
            .map_err(|_| anyhow!("mempool lock poisoned"))
    }

    fn consensus_read(&self) -> Result<RwLockReadGuard<'_, Box<dyn ConsensusEngine>>> {
        self.consensus
            .read()
            .map_err(|_| anyhow!("consensus lock poisoned"))
    }

    fn account_to_value(account: Account) -> Value {
        json!({
            "address": format!("{}", account.address),
            "balance": account.balance.to_string(),
            "nonce": account.nonce,
            "code_hash": account.code_hash.map(|hash| format!("{:?}", hash)),
            "storage_root": format!("{:?}", account.storage_root),
        })
    }

    fn maybe_strip_transactions(mut block: Block, include_full: bool) -> Block {
        if !include_full {
            block.transactions.clear();
        }
        block
    }

    fn decode_hash(value: &str) -> Result<H256> {
        let bytes = hex::decode(value.trim_start_matches("0x"))
            .context("provided hash is not valid hexadecimal")?;
        H256::from_slice(&bytes).map_err(|_| anyhow!("hash must be 32 bytes"))
    }
}

impl RpcBackend for NodeRpcBackend {
    fn send_raw_transaction(&self, tx_bytes: Vec<u8>) -> Result<H256> {
        let tx: Transaction =
            bincode::deserialize(&tx_bytes).context("failed to decode raw transaction bytes")?;

        tx.verify_signature()
            .context("transaction signature verification failed")?;
        tx.calculate_fee()
            .context("transaction fee validation failed")?;

        let tx_hash = tx.hash();

        let mut mempool = self.mempool_write()?;
        mempool
            .add_transaction(tx)
            .context("mempool rejected transaction")?;

        Ok(tx_hash)
    }

    fn get_block_by_number(&self, block_number: u64, full_tx: bool) -> Result<Option<Block>> {
        let block = self
            .chain_store
            .get_block_by_slot(block_number)
            .context("failed to read block from storage")?;
        Ok(block.map(|block| Self::maybe_strip_transactions(block, full_tx)))
    }

    fn get_block_by_hash(&self, block_hash: H256, full_tx: bool) -> Result<Option<Block>> {
        let block = self
            .chain_store
            .get_block_by_hash(&block_hash)
            .context("failed to read block from storage")?;
        Ok(block.map(|block| Self::maybe_strip_transactions(block, full_tx)))
    }

    fn get_transaction_receipt(&self, tx_hash: H256) -> Result<Option<TransactionReceipt>> {
        self.chain_store
            .get_receipt(&tx_hash)
            .context("failed to read transaction receipt from storage")
    }

    fn get_state_root(&self, block_ref: Option<String>) -> Result<H256> {
        match block_ref.as_deref() {
            None | Some("latest") => {
                let mut ledger = self.ledger_write()?;
                Ok(ledger.state_root())
            }
            Some("finalized") => {
                let consensus = self.consensus_read()?;
                let slot = consensus.finalized_slot();
                drop(consensus);

                let block = self
                    .chain_store
                    .get_block_by_slot(slot)
                    .context("failed to load finalized block")?
                    .ok_or_else(|| anyhow!("no block found for finalized slot {}", slot))?;
                Ok(block.header.state_root)
            }
            Some(reference) => {
                if let Ok(slot) = reference.parse::<u64>() {
                    let block = self
                        .chain_store
                        .get_block_by_slot(slot)
                        .context("failed to read block by slot")?
                        .ok_or_else(|| anyhow!("block {} not found", slot))?;
                    return Ok(block.header.state_root);
                }

                let hash = Self::decode_hash(reference)?;
                let block = self
                    .chain_store
                    .get_block_by_hash(&hash)
                    .context("failed to read block by hash")?
                    .ok_or_else(|| anyhow!("block {} not found", reference))?;
                Ok(block.header.state_root)
            }
        }
    }

    fn get_account(&self, address: Address, _block_ref: Option<String>) -> Result<Option<Value>> {
        let ledger = self.ledger_read()?;
        let account = ledger
            .get_account(&address)
            .context("failed to read account from ledger")?;
        Ok(account.map(Self::account_to_value))
    }

    fn get_slot_number(&self) -> Result<u64> {
        let consensus = self.consensus_read()?;
        Ok(consensus.current_slot())
    }

    fn get_finalized_slot(&self) -> Result<u64> {
        let consensus = self.consensus_read()?;
        Ok(consensus.finalized_slot())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_primitives::Keypair;
    use aether_state_storage::{Storage, StorageBatch, CF_ACCOUNTS};
    use aether_types::{Account, PublicKey, Signature, TransactionStatus, Vote, VrfProof};
    use std::collections::HashSet;
    use tempfile::TempDir;

    struct MockConsensus {
        current_slot: u64,
        finalized_slot: u64,
        total_stake: u128,
    }

    impl MockConsensus {
        fn new() -> Self {
            Self {
                current_slot: 12,
                finalized_slot: 8,
                total_stake: 1_000,
            }
        }
    }

    impl ConsensusEngine for MockConsensus {
        fn current_slot(&self) -> u64 {
            self.current_slot
        }

        fn advance_slot(&mut self) {
            self.current_slot += 1;
        }

        fn is_leader(&self, _slot: u64, _validator_pubkey: &PublicKey) -> bool {
            true
        }

        fn validate_block(&self, _block: &Block) -> Result<()> {
            Ok(())
        }

        fn add_vote(&mut self, _vote: Vote) -> Result<()> {
            Ok(())
        }

        fn check_finality(&mut self, slot: u64) -> bool {
            if slot > self.finalized_slot {
                self.finalized_slot = slot;
            }
            true
        }

        fn finalized_slot(&self) -> u64 {
            self.finalized_slot
        }

        fn total_stake(&self) -> u128 {
            self.total_stake
        }

        fn get_leader_proof(&self, _slot: u64) -> Option<VrfProof> {
            None
        }

        fn create_vote(&self, _block_hash: H256) -> Result<Option<Vote>> {
            Ok(None)
        }
    }

    struct TestContext {
        #[allow(dead_code)]
        temp_dir: TempDir,
        ledger: Arc<RwLock<Ledger>>,
        mempool: Arc<RwLock<Mempool>>,
        consensus: Arc<RwLock<Box<dyn ConsensusEngine>>>,
        chain_store: Arc<ChainStore>,
    }

    fn setup_backend() -> (NodeRpcBackend, TestContext) {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let ledger = Ledger::new(storage.clone()).unwrap();
        let chain_store = ChainStore::new(storage);

        let ledger_arc = Arc::new(RwLock::new(ledger));
        let mempool_arc = Arc::new(RwLock::new(Mempool::new()));
        let consensus_arc: Arc<RwLock<Box<dyn ConsensusEngine>>> =
            Arc::new(RwLock::new(Box::new(MockConsensus::new())));
        let chain_store_arc = Arc::new(chain_store);

        let backend = NodeRpcBackend::new(
            Arc::clone(&ledger_arc),
            Arc::clone(&mempool_arc),
            Arc::clone(&consensus_arc),
            Arc::clone(&chain_store_arc),
        );

        (
            backend,
            TestContext {
                temp_dir,
                ledger: ledger_arc,
                mempool: mempool_arc,
                consensus: consensus_arc,
                chain_store: chain_store_arc,
            },
        )
    }

    fn sample_transaction() -> Transaction {
        let keypair = Keypair::generate();
        let sender_pubkey = PublicKey::from_bytes(keypair.public_key());
        let sender = sender_pubkey.to_address();
        let mut tx = Transaction {
            nonce: 0,
            sender,
            sender_pubkey,
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 200_000,
            signature: Signature::from_bytes(vec![]),
        };

        let hash = tx.hash();
        let signature = keypair.sign(hash.as_bytes());
        tx.signature = Signature::from_bytes(signature);
        tx
    }

    #[test]
    fn reports_slots_from_consensus() {
        let (backend, _ctx) = setup_backend();
        assert_eq!(backend.get_slot_number().unwrap(), 12);
        assert_eq!(backend.get_finalized_slot().unwrap(), 8);
    }

    #[test]
    fn adds_transactions_to_mempool() {
        let (backend, ctx) = setup_backend();
        let tx = sample_transaction();
        let tx_hash = tx.hash();
        let bytes = bincode::serialize(&tx).unwrap();

        let returned = backend.send_raw_transaction(bytes).unwrap();
        assert_eq!(returned, tx_hash);

        let mempool = ctx.mempool.read().unwrap();
        assert_eq!(mempool.len(), 1);
    }

    #[test]
    fn fetches_account_from_ledger() {
        let (backend, ctx) = setup_backend();
        let address = Address::from_slice(&[2u8; 20]).unwrap();
        let account = Account::with_balance(address, 750_000);

        {
            let ledger = ctx.ledger.read().unwrap();
            let storage = ledger.storage();
            let mut batch = StorageBatch::new();
            batch.put(
                CF_ACCOUNTS,
                address.as_bytes().to_vec(),
                bincode::serialize(&account).unwrap(),
            );
            storage.write_batch(batch).unwrap();
        }

        let result = backend.get_account(address, None).unwrap();
        let json = result.expect("account exists");
        assert_eq!(json["balance"], "750000");
        assert_eq!(json["nonce"], 0);
    }

    #[test]
    fn retrieves_blocks_and_state_roots() {
        let (backend, ctx) = setup_backend();
        let proposer = Address::from_slice(&[9u8; 20]).unwrap();
        let mut block = Block::new(
            8,
            H256::zero(),
            proposer,
            VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            vec![],
        );
        block.header.state_root = H256::from_slice(&[4u8; 32]).unwrap();

        let block_hash = block.hash();
        ctx.chain_store
            .store_block(&block, &[])
            .expect("store block");

        let fetched = backend
            .get_block_by_number(8, false)
            .unwrap()
            .expect("block by number");
        assert_eq!(fetched.header.state_root, block.header.state_root);
        assert!(fetched.transactions.is_empty());

        let fetched_hash = backend
            .get_block_by_hash(block_hash, true)
            .unwrap()
            .expect("block by hash");
        assert_eq!(fetched_hash.header.slot, 8);

        let root_from_hash = backend
            .get_state_root(Some(format!("{:?}", block_hash)))
            .unwrap();
        assert_eq!(root_from_hash, block.header.state_root);

        let root_from_slot = backend.get_state_root(Some("8".to_string())).unwrap();
        assert_eq!(root_from_slot, block.header.state_root);
    }
}
