use aether_metrics::STORAGE_METRICS;
use anyhow::{Context, Result};
use rocksdb::{BlockBasedOptions, Cache, ColumnFamilyDescriptor, Options, WriteBatch, DB};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub const CF_ACCOUNTS: &str = "accounts";
pub const CF_UTXOS: &str = "utxos";
pub const CF_MERKLE: &str = "merkle_nodes";
pub const CF_BLOCKS: &str = "blocks";
pub const CF_RECEIPTS: &str = "receipts";
pub const CF_METADATA: &str = "metadata";
/// Tracks spent UTXOs by slot for light-client fraud proofs and audit.
/// Key: 8-byte big-endian slot + serialized UtxoId. Value: serialized SpentUtxoRecord.
/// Pruned at epoch boundaries based on retention_epochs.
pub const CF_SPENT_UTXOS: &str = "spent_utxos";
/// Persists the staking state (validators, delegations, unbonding queue) so that
/// slashing effects survive node restarts. Single key: "staking_state".
pub const CF_STAKING: &str = "staking";

type DbIterator<'a> = Box<dyn Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a>;

pub struct Storage {
    db: Arc<DB>,
    #[allow(dead_code)]
    block_cache: Cache,
}

impl Storage {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        // Global performance tuning
        opts.set_write_buffer_size(256 * 1024 * 1024); // 256MB
        opts.set_max_write_buffer_number(4);
        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        opts.increase_parallelism(num_cpus::get() as i32);
        opts.set_max_background_jobs(4);

        // Shared block cache (1GB) across all column families
        let block_cache = Cache::new_lru_cache(1024 * 1024 * 1024);

        let cfs = vec![
            ColumnFamilyDescriptor::new(CF_ACCOUNTS, Self::accounts_opts(&block_cache)),
            ColumnFamilyDescriptor::new(CF_UTXOS, Self::utxos_opts(&block_cache)),
            ColumnFamilyDescriptor::new(CF_MERKLE, Self::merkle_opts(&block_cache)),
            ColumnFamilyDescriptor::new(CF_BLOCKS, Self::blocks_opts(&block_cache)),
            ColumnFamilyDescriptor::new(CF_RECEIPTS, Self::receipts_opts(&block_cache)),
            ColumnFamilyDescriptor::new(CF_METADATA, Self::metadata_opts(&block_cache)),
            ColumnFamilyDescriptor::new(CF_SPENT_UTXOS, Self::spent_utxos_opts(&block_cache)),
            ColumnFamilyDescriptor::new(CF_STAKING, Self::metadata_opts(&block_cache)),
        ];

        let db = DB::open_cf_descriptors(&opts, path, cfs).context("failed to open database")?;

        Ok(Storage {
            db: Arc::new(db),
            block_cache,
        })
    }

    /// Accounts CF: point lookups dominate. Bloom filters + optimize for reads.
    fn accounts_opts(cache: &Cache) -> Options {
        let mut opts = Options::default();
        let mut bb = BlockBasedOptions::default();
        bb.set_bloom_filter(10.0, false); // 10 bits/key, full filter
        bb.set_block_cache(cache);
        bb.set_cache_index_and_filter_blocks(true);
        bb.set_block_size(16 * 1024); // 16KB blocks — small account records
        opts.set_block_based_table_factory(&bb);
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        // NOTE: do NOT call optimize_for_point_lookup() here — it internally
        // replaces the block-based table factory, discarding our bloom filter
        // and cache settings configured above.
        opts.set_write_buffer_size(64 * 1024 * 1024); // 64MB memtable
        opts
    }

    /// UTXOs CF: similar to accounts (point lookups for spend checks).
    fn utxos_opts(cache: &Cache) -> Options {
        let mut opts = Options::default();
        let mut bb = BlockBasedOptions::default();
        bb.set_bloom_filter(10.0, false);
        bb.set_block_cache(cache);
        bb.set_cache_index_and_filter_blocks(true);
        opts.set_block_based_table_factory(&bb);
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        opts
    }

    /// Merkle nodes CF: heavy reads during proof generation. Large cache.
    fn merkle_opts(cache: &Cache) -> Options {
        let mut opts = Options::default();
        let mut bb = BlockBasedOptions::default();
        bb.set_bloom_filter(10.0, false);
        bb.set_block_cache(cache);
        bb.set_cache_index_and_filter_blocks(true);
        bb.set_block_size(16 * 1024); // 16KB blocks (merkle nodes are small)
        opts.set_block_based_table_factory(&bb);
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        opts
    }

    /// Blocks CF: write-heavy, sequential access. Larger write buffer.
    fn blocks_opts(cache: &Cache) -> Options {
        let mut opts = Options::default();
        let mut bb = BlockBasedOptions::default();
        bb.set_block_cache(cache);
        bb.set_block_size(64 * 1024); // 64KB blocks (blocks are large)
        opts.set_block_based_table_factory(&bb);
        opts.set_compression_type(rocksdb::DBCompressionType::Zstd);
        opts.set_write_buffer_size(128 * 1024 * 1024); // 128MB
        opts
    }

    /// Receipts CF: append-only, rarely read. Compress aggressively.
    fn receipts_opts(cache: &Cache) -> Options {
        let mut opts = Options::default();
        let mut bb = BlockBasedOptions::default();
        bb.set_block_cache(cache);
        opts.set_block_based_table_factory(&bb);
        opts.set_compression_type(rocksdb::DBCompressionType::Zstd);
        opts
    }

    /// Spent UTXOs CF: append-heavy, prefix-scanned by slot during pruning.
    /// Keys are slot (8-byte BE) + utxo_id, enabling efficient range deletes.
    fn spent_utxos_opts(cache: &Cache) -> Options {
        let mut opts = Options::default();
        let mut bb = BlockBasedOptions::default();
        bb.set_block_cache(cache);
        opts.set_block_based_table_factory(&bb);
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        opts
    }

    /// Metadata CF: tiny, high read. Keep everything cached.
    fn metadata_opts(cache: &Cache) -> Options {
        let mut opts = Options::default();
        let mut bb = BlockBasedOptions::default();
        bb.set_block_cache(cache);
        bb.set_cache_index_and_filter_blocks(true);
        opts.set_block_based_table_factory(&bb);
        opts
    }

    pub fn get(&self, cf: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let cf_handle = self.db.cf_handle(cf).context("column family not found")?;
        let start = Instant::now();
        let result = self.db.get_cf(cf_handle, key)?;
        STORAGE_METRICS
            .read_latency_ms
            .observe(start.elapsed().as_secs_f64() * 1000.0);
        Ok(result)
    }

    pub fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let cf_handle = self.db.cf_handle(cf).context("column family not found")?;
        self.db.put_cf(cf_handle, key, value)?;
        Ok(())
    }

    pub fn delete(&self, cf: &str, key: &[u8]) -> Result<()> {
        let cf_handle = self.db.cf_handle(cf).context("column family not found")?;
        self.db.delete_cf(cf_handle, key)?;
        Ok(())
    }

    pub fn write_batch(&self, batch: StorageBatch) -> Result<()> {
        let mut wb = WriteBatch::default();
        let mut total_bytes: u64 = 0;

        for op in batch.operations {
            match op {
                BatchOperation::Put { cf, key, value } => {
                    let cf_handle = self.db.cf_handle(&cf).context("column family not found")?;
                    total_bytes += (key.len() + value.len()) as u64;
                    wb.put_cf(cf_handle, key, value);
                }
                BatchOperation::Delete { cf, key } => {
                    let cf_handle = self.db.cf_handle(&cf).context("column family not found")?;
                    wb.delete_cf(cf_handle, key);
                }
            }
        }

        let start = Instant::now();
        self.db.write(wb)?;
        STORAGE_METRICS
            .write_batch_ms
            .observe(start.elapsed().as_secs_f64() * 1000.0);
        if total_bytes > 0 {
            STORAGE_METRICS.bytes_written.inc_by(total_bytes);
        }
        Ok(())
    }

    pub fn iterator(&self, cf: &str) -> Result<DbIterator<'_>> {
        let cf_handle = self.db.cf_handle(cf).context("column family not found")?;
        let iter = self
            .db
            .iterator_cf(cf_handle, rocksdb::IteratorMode::Start)
            .filter_map(|item| item.ok());
        Ok(Box::new(iter))
    }

    /// Iterate all keys in a column family that start with the given prefix.
    pub fn prefix_iterator<'a>(&'a self, cf: &str, prefix: &[u8]) -> Result<DbIterator<'a>> {
        let cf_handle = self.db.cf_handle(cf).context("column family not found")?;
        let prefix_owned = prefix.to_vec();
        let iter = self
            .db
            .iterator_cf(
                cf_handle,
                rocksdb::IteratorMode::From(prefix, rocksdb::Direction::Forward),
            )
            .filter_map(|item| item.ok())
            .take_while(move |(k, _)| k.starts_with(&prefix_owned));
        Ok(Box::new(iter))
    }

    /// Delete all keys in a column family that match a prefix.
    /// Used for state pruning (e.g., deleting old blocks/receipts).
    pub fn delete_range(&self, cf: &str, start: &[u8], end: &[u8]) -> Result<()> {
        let cf_handle = self.db.cf_handle(cf).context("column family not found")?;
        let mut batch = WriteBatch::default();
        batch.delete_range_cf(cf_handle, start, end);
        self.db.write(batch)?;
        Ok(())
    }

    /// Trigger manual compaction on a column family.
    /// Call after bulk deletes to reclaim disk space.
    pub fn compact(&self, cf: &str) -> Result<()> {
        let cf_handle = self.db.cf_handle(cf).context("column family not found")?;
        self.db
            .compact_range_cf(cf_handle, None::<&[u8]>, None::<&[u8]>);
        Ok(())
    }

    /// Flush all in-memory WAL data to stable storage.
    ///
    /// Called during graceful shutdown to ensure all pending writes are
    /// durable before the process exits. Without this, a clean shutdown
    /// could still lose data sitting in the WAL buffer.
    pub fn flush_wal(&self) -> Result<()> {
        self.db.flush_wal(true).context("failed to flush WAL")?;
        Ok(())
    }
}

pub struct StorageBatch {
    operations: Vec<BatchOperation>,
}

enum BatchOperation {
    Put {
        cf: String,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        cf: String,
        key: Vec<u8>,
    },
}

impl StorageBatch {
    pub fn new() -> Self {
        StorageBatch {
            operations: Vec::new(),
        }
    }

    pub fn put(&mut self, cf: &str, key: Vec<u8>, value: Vec<u8>) {
        self.operations.push(BatchOperation::Put {
            cf: cf.to_string(),
            key,
            value,
        });
    }

    pub fn delete(&mut self, cf: &str, key: Vec<u8>) {
        self.operations.push(BatchOperation::Delete {
            cf: cf.to_string(),
            key,
        });
    }

    /// Merge all operations from another batch into this one.
    /// Used to combine multiple logical writes into a single atomic commit.
    pub fn extend(&mut self, other: StorageBatch) {
        self.operations.extend(other.operations);
    }
}

impl Default for StorageBatch {
    fn default() -> Self {
        Self::new()
    }
}

/// State pruning utilities.
///
/// Blocks are keyed by hash in CF_BLOCKS with a `slot:{N}` → hash index in
/// CF_METADATA.  Receipts are keyed by tx_hash in CF_RECEIPTS.  Pruning
/// therefore cannot use `delete_range` on these CFs directly.  Instead we
/// scan the slot index, load each block to discover its tx hashes, and
/// delete all related entries in a single atomic WriteBatch.
pub mod pruning {
    use super::*;

    /// Prune blocks, their receipts, and slot-index entries for all slots
    /// below `min_slot`.  Returns the number of blocks pruned.
    pub fn prune_old_blocks_and_receipts(storage: &Storage, min_slot: u64) -> Result<u64> {
        let mut batch = StorageBatch::new();
        let mut pruned = 0u64;

        // Scan CF_METADATA for "slot:" keys.  Keys are UTF-8 strings like "slot:42".
        for (key_bytes, hash_bytes) in storage.prefix_iterator(CF_METADATA, b"slot:")? {
            let key_str = match std::str::from_utf8(&key_bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let slot: u64 = match key_str.strip_prefix("slot:").and_then(|s| s.parse().ok()) {
                Some(s) => s,
                None => continue,
            };
            if slot >= min_slot {
                // Decimal string keys are not numerically ordered ("slot:9" > "slot:10"),
                // so we cannot break early — just skip slots above the threshold.
                continue;
            }

            // Delete the block from CF_BLOCKS (keyed by hash).
            batch.delete(CF_BLOCKS, hash_bytes.to_vec());

            // Load the block to find tx hashes for receipt pruning.
            if let Ok(Some(block_bytes)) = storage.get(CF_BLOCKS, &hash_bytes) {
                if let Ok(block) = bincode::deserialize::<aether_types::Block>(&block_bytes) {
                    for tx in &block.transactions {
                        let tx_hash = tx.hash();
                        batch.delete(CF_RECEIPTS, tx_hash.as_bytes().to_vec());
                    }
                }
            }

            // Delete the slot index entry itself.
            batch.delete(CF_METADATA, key_bytes.to_vec());
            pruned += 1;
        }

        if pruned > 0 {
            storage.write_batch(batch)?;
            storage.compact(CF_BLOCKS)?;
            storage.compact(CF_RECEIPTS)?;
        }

        Ok(pruned)
    }

    // Keep the old function names as thin wrappers so existing callers compile.
    // Both now route through the combined function.

    /// Prune old blocks (and their receipts) from storage.
    pub fn prune_old_blocks(storage: &Storage, min_slot: u64) -> Result<u64> {
        prune_old_blocks_and_receipts(storage, min_slot)
    }

    /// Prune old receipts from storage.
    ///
    /// Receipts are pruned together with their blocks by `prune_old_blocks`.
    /// This function exists for backward compatibility and is a no-op.
    pub fn prune_old_receipts(storage: &Storage, _min_slot: u64) -> Result<u64> {
        let _ = storage;
        Ok(0)
    }

    /// Prune spent-UTXO records for all slots below `min_slot`.
    ///
    /// CF_SPENT_UTXOS keys are prefixed with an 8-byte big-endian slot number.
    /// Iterates all entries, collects those with slot < min_slot, and deletes
    /// them in a single WriteBatch.  After deletion, compacts CF_SPENT_UTXOS
    /// and CF_UTXOS to reclaim disk space from tombstones.
    ///
    /// Returns the number of spent-UTXO records pruned.
    pub fn prune_spent_utxos(storage: &Storage, min_slot: u64) -> Result<u64> {
        let mut batch = StorageBatch::new();
        let mut count = 0u64;

        // Keys are 8-byte BE slot + utxo_id.  Because they're big-endian,
        // the iterator returns them in ascending slot order — we can break
        // early once we pass the threshold.
        for (key, _) in storage.iterator(CF_SPENT_UTXOS)? {
            if key.len() < 8 {
                continue;
            }
            let slot_bytes: [u8; 8] = key[..8].try_into().unwrap_or([0; 8]);
            let slot = u64::from_be_bytes(slot_bytes);
            if slot >= min_slot {
                break;
            }
            batch.delete(CF_SPENT_UTXOS, key.to_vec());
            count += 1;
        }

        if count > 0 {
            storage.write_batch(batch)?;
            storage.compact(CF_SPENT_UTXOS)?;
        }

        // Also compact the UTXO column family to reclaim space from
        // tombstones left by regular UTXO consumption (spend = delete).
        storage.compact(CF_UTXOS)?;

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_basic_operations() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();

        storage.put(CF_METADATA, b"key1", b"value1").unwrap();
        let value = storage.get(CF_METADATA, b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        storage.delete(CF_METADATA, b"key1").unwrap();
        let value = storage.get(CF_METADATA, b"key1").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_batch_write() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();

        let mut batch = StorageBatch::new();
        batch.put(CF_METADATA, b"key1".to_vec(), b"value1".to_vec());
        batch.put(CF_METADATA, b"key2".to_vec(), b"value2".to_vec());

        storage.write_batch(batch).unwrap();

        assert_eq!(
            storage.get(CF_METADATA, b"key1").unwrap(),
            Some(b"value1".to_vec())
        );
        assert_eq!(
            storage.get(CF_METADATA, b"key2").unwrap(),
            Some(b"value2".to_vec())
        );
    }

    #[test]
    fn test_bloom_filter_accounts() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();

        // Write 1000 accounts
        for i in 0u32..1000 {
            let key = i.to_be_bytes();
            let value = format!("account_{}", i);
            storage.put(CF_ACCOUNTS, &key, value.as_bytes()).unwrap();
        }

        // Point lookup should be fast with bloom filter
        let val = storage.get(CF_ACCOUNTS, &500u32.to_be_bytes()).unwrap();
        assert!(val.is_some());

        // Non-existent key — bloom filter avoids disk read
        let val = storage.get(CF_ACCOUNTS, &9999u32.to_be_bytes()).unwrap();
        assert!(val.is_none());
    }

    #[test]
    fn test_pruning_blocks_and_receipts() {
        use aether_types::{Address, Block, BlockHeader, VrfProof, H256};

        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();

        // Store 10 blocks using the production key layout:
        //   CF_BLOCKS: hash → block bytes
        //   CF_METADATA: "slot:{N}" → hash
        let mut hashes = Vec::new();
        for slot in 0u64..10 {
            let block = Block {
                header: BlockHeader {
                    version: 1,
                    slot,
                    parent_hash: H256::zero(),
                    state_root: H256::zero(),
                    transactions_root: H256::zero(),
                    receipts_root: H256::zero(),
                    proposer: Address::from_slice(&[0u8; 20]).unwrap(),
                    vrf_proof: VrfProof {
                        output: [0u8; 32],
                        proof: vec![],
                    },
                    timestamp: 0,
                },
                transactions: vec![],
                aggregated_vote: None,
                slash_evidence: vec![],
            };
            let hash = block.hash();
            let block_bytes = bincode::serialize(&block).unwrap();
            hashes.push(hash);

            storage
                .put(CF_BLOCKS, hash.as_bytes(), &block_bytes)
                .unwrap();
            let slot_key = format!("slot:{}", slot);
            storage
                .put(CF_METADATA, slot_key.as_bytes(), hash.as_bytes())
                .unwrap();
        }

        // Verify slot 3 block exists
        assert!(storage
            .get(CF_BLOCKS, hashes[3].as_bytes())
            .unwrap()
            .is_some());

        // Prune slots < 5
        let pruned = pruning::prune_old_blocks(&storage, 5).unwrap();
        assert_eq!(pruned, 5);

        // Slots 0-4 should be gone
        for (slot, hash) in hashes.iter().enumerate().take(5) {
            assert!(
                storage.get(CF_BLOCKS, hash.as_bytes()).unwrap().is_none(),
                "slot {} block should be pruned",
                slot
            );
            let key = format!("slot:{}", slot);
            assert!(
                storage.get(CF_METADATA, key.as_bytes()).unwrap().is_none(),
                "slot {} index should be pruned",
                slot
            );
        }

        // Slots 5-9 should still exist
        for (slot, hash) in hashes.iter().enumerate().skip(5) {
            assert!(
                storage.get(CF_BLOCKS, hash.as_bytes()).unwrap().is_some(),
                "slot {} block should still exist",
                slot
            );
        }
    }

    #[test]
    fn test_prune_spent_utxos() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();

        // Write spent-UTXO records at various slots.
        // Key format: 8-byte BE slot + arbitrary utxo_id bytes.
        for slot in 0u64..10 {
            let mut key = slot.to_be_bytes().to_vec();
            key.extend_from_slice(&[slot as u8; 16]); // fake utxo_id
            storage.put(CF_SPENT_UTXOS, &key, b"").unwrap();
        }

        // Verify all 10 exist
        let mut count = 0;
        for _ in storage.iterator(CF_SPENT_UTXOS).unwrap() {
            count += 1;
        }
        assert_eq!(count, 10);

        // Prune slots < 5
        let pruned = pruning::prune_spent_utxos(&storage, 5).unwrap();
        assert_eq!(pruned, 5);

        // Verify only slots 5-9 remain
        let mut remaining_slots = Vec::new();
        for (key, _) in storage.iterator(CF_SPENT_UTXOS).unwrap() {
            let slot = u64::from_be_bytes(key[..8].try_into().unwrap());
            remaining_slots.push(slot);
        }
        assert_eq!(remaining_slots, vec![5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_prune_spent_utxos_empty() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();

        // Pruning an empty CF should return 0 and not error.
        let pruned = pruning::prune_spent_utxos(&storage, 100).unwrap();
        assert_eq!(pruned, 0);
    }

    #[test]
    fn test_flush_wal_durability() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();

        // Write data and flush WAL
        storage
            .put(CF_METADATA, b"flush_key", b"flush_value")
            .unwrap();
        storage.flush_wal().unwrap();

        // Re-open the database — flushed data should be durable
        drop(storage);
        let storage2 = Storage::open(temp_dir.path()).unwrap();
        let value = storage2.get(CF_METADATA, b"flush_key").unwrap();
        assert_eq!(value, Some(b"flush_value".to_vec()));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use tempfile::TempDir;

    /// Arbitrary non-empty byte vector (keys and values).
    fn arb_bytes() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(any::<u8>(), 1..64)
    }

    proptest! {
        /// A value written to any supported CF can always be read back unchanged.
        #[test]
        fn write_read_roundtrip(
            key in arb_bytes(),
            value in arb_bytes(),
            cf_idx in 0usize..4,
        ) {
            let cfs = [CF_ACCOUNTS, CF_UTXOS, CF_BLOCKS, CF_METADATA];
            let cf = cfs[cf_idx];
            let tmp = TempDir::new().unwrap();
            let storage = Storage::open(tmp.path()).unwrap();
            storage.put(cf, &key, &value).unwrap();
            let result = storage.get(cf, &key).unwrap();
            prop_assert_eq!(result, Some(value));
        }

        /// A key that was never written returns None.
        #[test]
        fn absent_key_returns_none(key in arb_bytes()) {
            let tmp = TempDir::new().unwrap();
            let storage = Storage::open(tmp.path()).unwrap();
            let result = storage.get(CF_METADATA, &key).unwrap();
            prop_assert_eq!(result, None);
        }

        /// After deleting a key, get returns None.
        #[test]
        fn delete_removes_key(key in arb_bytes(), value in arb_bytes()) {
            let tmp = TempDir::new().unwrap();
            let storage = Storage::open(tmp.path()).unwrap();
            storage.put(CF_METADATA, &key, &value).unwrap();
            storage.delete(CF_METADATA, &key).unwrap();
            let result = storage.get(CF_METADATA, &key).unwrap();
            prop_assert_eq!(result, None);
        }

        /// Writing a key twice returns the latest value (last-write-wins).
        #[test]
        fn overwrite_returns_latest(key in arb_bytes(), v1 in arb_bytes(), v2 in arb_bytes()) {
            let tmp = TempDir::new().unwrap();
            let storage = Storage::open(tmp.path()).unwrap();
            storage.put(CF_METADATA, &key, &v1).unwrap();
            storage.put(CF_METADATA, &key, &v2).unwrap();
            let result = storage.get(CF_METADATA, &key).unwrap();
            prop_assert_eq!(result, Some(v2));
        }

        /// All writes in a batch are atomically visible after write_batch.
        #[test]
        fn batch_all_writes_visible(
            pairs in prop::collection::vec((arb_bytes(), arb_bytes()), 1..10),
        ) {
            let tmp = TempDir::new().unwrap();
            let storage = Storage::open(tmp.path()).unwrap();
            let mut batch = StorageBatch::new();
            for (k, v) in &pairs {
                batch.put(CF_METADATA, k.clone(), v.clone());
            }
            storage.write_batch(batch).unwrap();
            for (k, v) in &pairs {
                let result = storage.get(CF_METADATA, k).unwrap();
                prop_assert_eq!(&result, &Some(v.clone()));
            }
        }

        /// Batch delete removes all targeted keys and leaves others untouched.
        #[test]
        fn batch_delete_selective(
            key_del in arb_bytes(),
            key_keep in arb_bytes(),
            val_del in arb_bytes(),
            val_keep in arb_bytes(),
        ) {
            prop_assume!(key_del != key_keep);
            let tmp = TempDir::new().unwrap();
            let storage = Storage::open(tmp.path()).unwrap();

            // Write both
            storage.put(CF_METADATA, &key_del, &val_del).unwrap();
            storage.put(CF_METADATA, &key_keep, &val_keep).unwrap();

            // Delete one via batch
            let mut batch = StorageBatch::new();
            batch.delete(CF_METADATA, key_del.clone());
            storage.write_batch(batch).unwrap();

            prop_assert_eq!(storage.get(CF_METADATA, &key_del).unwrap(), None);
            prop_assert_eq!(storage.get(CF_METADATA, &key_keep).unwrap(), Some(val_keep));
        }

        /// Writes to different column families are isolated — same key in different CFs
        /// holds independent values.
        #[test]
        fn column_family_isolation(key in arb_bytes(), v1 in arb_bytes(), v2 in arb_bytes()) {
            prop_assume!(v1 != v2);
            let tmp = TempDir::new().unwrap();
            let storage = Storage::open(tmp.path()).unwrap();
            storage.put(CF_ACCOUNTS, &key, &v1).unwrap();
            storage.put(CF_METADATA, &key, &v2).unwrap();
            prop_assert_eq!(storage.get(CF_ACCOUNTS, &key).unwrap(), Some(v1));
            prop_assert_eq!(storage.get(CF_METADATA, &key).unwrap(), Some(v2));
        }

        /// Keys stored in CF_BLOCKS with a numeric slot prefix are retrievable
        /// by exact lookup (simulating block storage pattern).
        #[test]
        fn slot_prefix_lookup(slot in any::<u64>(), suffix in arb_bytes(), value in arb_bytes()) {
            let tmp = TempDir::new().unwrap();
            let storage = Storage::open(tmp.path()).unwrap();
            let mut key = slot.to_be_bytes().to_vec();
            key.extend_from_slice(&suffix);
            storage.put(CF_BLOCKS, &key, &value).unwrap();
            let result = storage.get(CF_BLOCKS, &key).unwrap();
            prop_assert_eq!(result, Some(value));
        }

        /// Deleting a key that was never written does not error.
        #[test]
        fn delete_nonexistent_is_safe(key in arb_bytes()) {
            let tmp = TempDir::new().unwrap();
            let storage = Storage::open(tmp.path()).unwrap();
            prop_assert!(storage.delete(CF_METADATA, &key).is_ok());
        }

        /// StorageBatch::extend merges both batches: all keys from both are written.
        #[test]
        fn batch_extend_merges(
            pairs_a in prop::collection::vec((arb_bytes(), arb_bytes()), 1..5),
            pairs_b in prop::collection::vec((arb_bytes(), arb_bytes()), 1..5),
        ) {
            // Ensure no key overlap between batches (for deterministic result)
            prop_assume!(pairs_a.iter().all(|(ka, _)| pairs_b.iter().all(|(kb, _)| ka != kb)));

            let tmp = TempDir::new().unwrap();
            let storage = Storage::open(tmp.path()).unwrap();
            let mut batch_a = StorageBatch::new();
            let mut batch_b = StorageBatch::new();
            for (k, v) in &pairs_a {
                batch_a.put(CF_METADATA, k.clone(), v.clone());
            }
            for (k, v) in &pairs_b {
                batch_b.put(CF_METADATA, k.clone(), v.clone());
            }
            batch_a.extend(batch_b);
            storage.write_batch(batch_a).unwrap();

            for (k, v) in pairs_a.iter().chain(pairs_b.iter()) {
                let result = storage.get(CF_METADATA, k).unwrap();
                prop_assert_eq!(&result, &Some(v.clone()));
            }
        }
    }
}
