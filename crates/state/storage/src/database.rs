use anyhow::{Context, Result};
use rocksdb::{BlockBasedOptions, Cache, ColumnFamilyDescriptor, Options, WriteBatch, DB};
use std::path::Path;
use std::sync::Arc;

pub const CF_ACCOUNTS: &str = "accounts";
pub const CF_UTXOS: &str = "utxos";
pub const CF_MERKLE: &str = "merkle_nodes";
pub const CF_BLOCKS: &str = "blocks";
pub const CF_RECEIPTS: &str = "receipts";
pub const CF_METADATA: &str = "metadata";

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
        Ok(self.db.get_cf(cf_handle, key)?)
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

        for op in batch.operations {
            match op {
                BatchOperation::Put { cf, key, value } => {
                    let cf_handle = self.db.cf_handle(&cf).context("column family not found")?;
                    wb.put_cf(cf_handle, key, value);
                }
                BatchOperation::Delete { cf, key } => {
                    let cf_handle = self.db.cf_handle(&cf).context("column family not found")?;
                    wb.delete_cf(cf_handle, key);
                }
            }
        }

        self.db.write(wb)?;
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
pub mod pruning {
    use super::*;

    /// Prune old blocks from storage.
    ///
    /// Deletes all block entries with slot numbers below `min_slot`.
    /// Blocks are keyed by slot number (big-endian u64).
    pub fn prune_old_blocks(storage: &Storage, min_slot: u64) -> Result<u64> {
        let start = 0u64.to_be_bytes();
        let end = min_slot.to_be_bytes();
        storage.delete_range(CF_BLOCKS, &start, &end)?;
        storage.compact(CF_BLOCKS)?;
        Ok(min_slot)
    }

    /// Prune old receipts from storage.
    pub fn prune_old_receipts(storage: &Storage, min_slot: u64) -> Result<u64> {
        let start = 0u64.to_be_bytes();
        let end = min_slot.to_be_bytes();
        storage.delete_range(CF_RECEIPTS, &start, &end)?;
        storage.compact(CF_RECEIPTS)?;
        Ok(min_slot)
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
    fn test_pruning_blocks() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();

        // Write blocks for slots 0..100
        for slot in 0u64..100 {
            let key = slot.to_be_bytes();
            let value = format!("block_{}", slot);
            storage.put(CF_BLOCKS, &key, value.as_bytes()).unwrap();
        }

        // Verify slot 50 exists
        assert!(storage
            .get(CF_BLOCKS, &50u64.to_be_bytes())
            .unwrap()
            .is_some());

        // Prune slots < 50
        pruning::prune_old_blocks(&storage, 50).unwrap();

        // Slot 10 should be gone
        assert!(storage
            .get(CF_BLOCKS, &10u64.to_be_bytes())
            .unwrap()
            .is_none());

        // Slot 50 should still exist
        assert!(storage
            .get(CF_BLOCKS, &50u64.to_be_bytes())
            .unwrap()
            .is_some());

        // Slot 99 should still exist
        assert!(storage
            .get(CF_BLOCKS, &99u64.to_be_bytes())
            .unwrap()
            .is_some());
    }
}
