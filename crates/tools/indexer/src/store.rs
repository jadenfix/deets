use aether_types::{Block, H256};
use anyhow::{Context, Result};
use rocksdb::{ColumnFamilyDescriptor, Options, DB};
use serde::{Deserialize, Serialize};
use std::path::Path;

const CF_BLOCKS: &str = "indexed_blocks";
const CF_TX_INDEX: &str = "tx_to_slot";
const CF_META: &str = "indexer_meta";

/// Indexed block record stored in RocksDB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedBlock {
    pub slot: u64,
    pub hash: H256,
    pub tx_count: usize,
    pub proposer: String,
    pub timestamp: u64,
    pub tx_hashes: Vec<H256>,
}

/// Persistent indexer store backed by RocksDB.
pub struct PersistentStore {
    db: DB,
}

impl PersistentStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cfs = vec![
            ColumnFamilyDescriptor::new(CF_BLOCKS, Options::default()),
            ColumnFamilyDescriptor::new(CF_TX_INDEX, Options::default()),
            ColumnFamilyDescriptor::new(CF_META, Options::default()),
        ];

        let db =
            DB::open_cf_descriptors(&opts, path, cfs).context("failed to open indexer database")?;

        Ok(PersistentStore { db })
    }

    /// Ingest a block into the index.
    pub fn ingest(&self, block: &Block) -> Result<()> {
        let indexed = IndexedBlock {
            slot: block.header.slot,
            hash: block.hash(),
            tx_count: block.transactions.len(),
            proposer: format!("{:?}", block.header.proposer),
            timestamp: block.header.timestamp,
            tx_hashes: block.transactions.iter().map(|tx| tx.hash()).collect(),
        };

        let key = block.header.slot.to_be_bytes();
        let value = bincode::serialize(&indexed)?;

        let cf = self.db.cf_handle(CF_BLOCKS).context("missing blocks CF")?;
        self.db.put_cf(cf, key, value)?;

        // Index each transaction hash → slot
        let tx_cf = self
            .db
            .cf_handle(CF_TX_INDEX)
            .context("missing tx_index CF")?;
        for tx in &block.transactions {
            let tx_hash = tx.hash();
            self.db.put_cf(tx_cf, tx_hash.as_bytes(), key)?;
        }

        // Update latest slot
        let meta_cf = self.db.cf_handle(CF_META).context("missing meta CF")?;
        self.db.put_cf(meta_cf, b"latest_slot", key)?;

        Ok(())
    }

    /// Get an indexed block by slot number.
    pub fn get_block(&self, slot: u64) -> Result<Option<IndexedBlock>> {
        let cf = self.db.cf_handle(CF_BLOCKS).context("missing blocks CF")?;
        let key = slot.to_be_bytes();
        match self.db.get_cf(cf, key)? {
            Some(bytes) => Ok(Some(bincode::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Look up which slot a transaction belongs to.
    pub fn get_tx_slot(&self, tx_hash: &H256) -> Result<Option<u64>> {
        let cf = self
            .db
            .cf_handle(CF_TX_INDEX)
            .context("missing tx_index CF")?;
        match self.db.get_cf(cf, tx_hash.as_bytes())? {
            Some(bytes) => {
                let slot_bytes: [u8; 8] =
                    bytes.as_slice().try_into().context("invalid slot bytes")?;
                Ok(Some(u64::from_be_bytes(slot_bytes)))
            }
            None => Ok(None),
        }
    }

    /// Get the latest indexed slot.
    pub fn latest_slot(&self) -> Result<u64> {
        let cf = self.db.cf_handle(CF_META).context("missing meta CF")?;
        match self.db.get_cf(cf, b"latest_slot")? {
            Some(bytes) => {
                let slot_bytes: [u8; 8] =
                    bytes.as_slice().try_into().context("invalid slot bytes")?;
                Ok(u64::from_be_bytes(slot_bytes))
            }
            None => Ok(0),
        }
    }

    /// Count total indexed blocks (approximate — scans CF).
    pub fn block_count(&self) -> Result<usize> {
        let cf = self.db.cf_handle(CF_BLOCKS).context("missing blocks CF")?;
        let count = self
            .db
            .iterator_cf(cf, rocksdb::IteratorMode::Start)
            .count();
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::*;

    fn make_block(slot: u64, num_txs: usize) -> Block {
        let txs: Vec<Transaction> = (0..num_txs)
            .map(|i| Transaction {
                nonce: i as u64,
                chain_id: 1,
                sender: Address::from_slice(&[1u8; 20]).unwrap(),
                sender_pubkey: PublicKey::from_bytes(vec![2u8; 32]),
                inputs: vec![],
                outputs: vec![],
                reads: std::collections::HashSet::new(),
                writes: std::collections::HashSet::new(),
                program_id: None,
                data: vec![slot as u8, i as u8],
                gas_limit: 21000,
                fee: 1000,
                signature: Signature::from_bytes(vec![3u8; 64]),
            })
            .collect();

        Block::new(
            slot,
            H256::zero(),
            Address::from_slice(&[1u8; 20]).unwrap(),
            VrfProof {
                output: [0u8; 32],
                proof: vec![0u8; 80],
            },
            txs,
        )
    }

    #[test]
    fn test_ingest_and_query() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = PersistentStore::open(dir.path()).unwrap();

        let block = make_block(42, 3);
        store.ingest(&block).unwrap();

        let indexed = store.get_block(42).unwrap().unwrap();
        assert_eq!(indexed.slot, 42);
        assert_eq!(indexed.tx_count, 3);
    }

    #[test]
    fn test_tx_lookup() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = PersistentStore::open(dir.path()).unwrap();

        let block = make_block(10, 2);
        let tx_hash = block.transactions[0].hash();
        store.ingest(&block).unwrap();

        let slot = store.get_tx_slot(&tx_hash).unwrap();
        assert_eq!(slot, Some(10));
    }

    #[test]
    fn test_latest_slot_tracking() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = PersistentStore::open(dir.path()).unwrap();

        assert_eq!(store.latest_slot().unwrap(), 0);

        store.ingest(&make_block(5, 1)).unwrap();
        assert_eq!(store.latest_slot().unwrap(), 5);

        store.ingest(&make_block(10, 1)).unwrap();
        assert_eq!(store.latest_slot().unwrap(), 10);
    }

    #[test]
    fn test_persistence_across_reopen() {
        let dir = tempfile::TempDir::new().unwrap();

        {
            let store = PersistentStore::open(dir.path()).unwrap();
            store.ingest(&make_block(100, 5)).unwrap();
        }

        // Reopen and verify data persists
        let store = PersistentStore::open(dir.path()).unwrap();
        let block = store.get_block(100).unwrap().unwrap();
        assert_eq!(block.slot, 100);
        assert_eq!(block.tx_count, 5);
        assert_eq!(store.latest_slot().unwrap(), 100);
    }
}
