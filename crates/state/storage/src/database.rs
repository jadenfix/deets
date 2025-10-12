use anyhow::{Result, Context};
use rocksdb::{DB, Options, ColumnFamilyDescriptor, WriteBatch};
use std::path::Path;
use std::sync::Arc;

pub const CF_ACCOUNTS: &str = "accounts";
pub const CF_UTXOS: &str = "utxos";
pub const CF_MERKLE: &str = "merkle_nodes";
pub const CF_BLOCKS: &str = "blocks";
pub const CF_RECEIPTS: &str = "receipts";
pub const CF_METADATA: &str = "metadata";

pub struct Storage {
    db: Arc<DB>,
}

impl Storage {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        
        // Performance tuning
        opts.set_write_buffer_size(256 * 1024 * 1024); // 256MB
        opts.set_max_write_buffer_number(4);
        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        opts.increase_parallelism(num_cpus::get() as i32);
        
        let cfs = vec![
            ColumnFamilyDescriptor::new(CF_ACCOUNTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_UTXOS, Options::default()),
            ColumnFamilyDescriptor::new(CF_MERKLE, Options::default()),
            ColumnFamilyDescriptor::new(CF_BLOCKS, Options::default()),
            ColumnFamilyDescriptor::new(CF_RECEIPTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_METADATA, Options::default()),
        ];

        let db = DB::open_cf_descriptors(&opts, path, cfs)
            .context("failed to open database")?;

        Ok(Storage { db: Arc::new(db) })
    }

    pub fn get(&self, cf: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let cf_handle = self.db.cf_handle(cf)
            .context("column family not found")?;
        Ok(self.db.get_cf(cf_handle, key)?)
    }

    pub fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let cf_handle = self.db.cf_handle(cf)
            .context("column family not found")?;
        self.db.put_cf(cf_handle, key, value)?;
        Ok(())
    }

    pub fn delete(&self, cf: &str, key: &[u8]) -> Result<()> {
        let cf_handle = self.db.cf_handle(cf)
            .context("column family not found")?;
        self.db.delete_cf(cf_handle, key)?;
        Ok(())
    }

    pub fn write_batch(&self, batch: StorageBatch) -> Result<()> {
        let mut wb = WriteBatch::default();
        
        for op in batch.operations {
            match op {
                BatchOperation::Put { cf, key, value } => {
                    let cf_handle = self.db.cf_handle(&cf)
                        .context("column family not found")?;
                    wb.put_cf(cf_handle, key, value);
                }
                BatchOperation::Delete { cf, key } => {
                    let cf_handle = self.db.cf_handle(&cf)
                        .context("column family not found")?;
                    wb.delete_cf(cf_handle, key);
                }
            }
        }
        
        self.db.write(wb)?;
        Ok(())
    }

    pub fn iterator(&self, cf: &str) -> Result<impl Iterator<Item = (Box<[u8]>, Box<[u8]>)> + '_> {
        let cf_handle = self.db.cf_handle(cf)
            .context("column family not found")?;
        Ok(self.db.iterator_cf(cf_handle, rocksdb::IteratorMode::Start)
            .map(|item| item.unwrap()))
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

        assert_eq!(storage.get(CF_METADATA, b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(storage.get(CF_METADATA, b"key2").unwrap(), Some(b"value2".to_vec()));
    }
}

