use aether_state_storage::{Storage, StorageBatch, CF_BLOCKS, CF_METADATA, CF_RECEIPTS};
use aether_types::{Block, TransactionReceipt, H256};
use anyhow::{anyhow, Context, Result};

const SLOT_PREFIX: &[u8] = b"slot:";
const TIP_KEY: &[u8] = b"chain_tip";

#[derive(Clone)]
pub struct ChainStore {
    storage: Storage,
}

impl ChainStore {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub fn store_block(&self, block: &Block, receipts: &[TransactionReceipt]) -> Result<()> {
        let block_hash = block.hash();
        let serialized_block =
            bincode::serialize(block).context("failed to serialize block for storage")?;

        let mut batch = StorageBatch::new();
        batch.put(CF_BLOCKS, block_hash.as_bytes().to_vec(), serialized_block);
        batch.put(
            CF_METADATA,
            Self::slot_key(block.header.slot),
            block_hash.as_bytes().to_vec(),
        );
        batch.put(
            CF_METADATA,
            TIP_KEY.to_vec(),
            block_hash.as_bytes().to_vec(),
        );

        for receipt in receipts {
            if receipt.block_hash != block_hash {
                return Err(anyhow!(
                    "receipt {:#?} does not match block hash {}",
                    receipt.tx_hash,
                    block_hash
                ));
            }

            let encoded =
                bincode::serialize(receipt).context("failed to serialize transaction receipt")?;
            batch.put(CF_RECEIPTS, receipt.tx_hash.as_bytes().to_vec(), encoded);
        }

        self.storage
            .write_batch(batch)
            .context("failed to persist block data to storage")
    }

    pub fn get_block_by_hash(&self, hash: &H256) -> Result<Option<Block>> {
        match self.storage.get(CF_BLOCKS, hash.as_bytes())? {
            Some(bytes) => {
                let block: Block = bincode::deserialize(&bytes)
                    .context("failed to deserialize stored block data")?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    pub fn get_block_by_slot(&self, slot: u64) -> Result<Option<Block>> {
        if let Some(hash_bytes) = self.storage.get(CF_METADATA, &Self::slot_key(slot))? {
            let hash = H256::from_slice(&hash_bytes)
                .map_err(|_| anyhow!("invalid block hash stored for slot {slot}"))?;
            self.get_block_by_hash(&hash)
        } else {
            Ok(None)
        }
    }

    pub fn get_receipt(&self, tx_hash: &H256) -> Result<Option<TransactionReceipt>> {
        match self
            .storage
            .get(CF_RECEIPTS, tx_hash.as_bytes())
            .context("failed to read receipt from storage")?
        {
            Some(bytes) => {
                let receipt: TransactionReceipt = bincode::deserialize(&bytes)
                    .context("failed to deserialize stored transaction receipt")?;
                Ok(Some(receipt))
            }
            None => Ok(None),
        }
    }

    pub fn latest_block_hash(&self) -> Result<Option<H256>> {
        match self.storage.get(CF_METADATA, TIP_KEY)? {
            Some(bytes) => {
                let hash = H256::from_slice(&bytes)
                    .map_err(|_| anyhow!("invalid block hash stored for chain tip"))?;
                Ok(Some(hash))
            }
            None => Ok(None),
        }
    }

    fn slot_key(slot: u64) -> Vec<u8> {
        let mut key = Vec::with_capacity(SLOT_PREFIX.len() + std::mem::size_of::<u64>());
        key.extend_from_slice(SLOT_PREFIX);
        key.extend_from_slice(&slot.to_be_bytes());
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_state_storage::Storage;
    use aether_types::{Address, TransactionReceipt, TransactionStatus, VrfProof, H256};
    use tempfile::TempDir;

    fn sample_block(slot: u64) -> Block {
        let proposer = Address::from_slice(&[1u8; 20]).unwrap();
        Block::new(
            slot,
            H256::zero(),
            proposer,
            VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            vec![],
        )
    }

    fn sample_receipt(tx_hash: H256, block_hash: H256, slot: u64) -> TransactionReceipt {
        TransactionReceipt {
            tx_hash,
            block_hash,
            slot,
            status: TransactionStatus::Success,
            gas_used: 0,
            logs: vec![],
            state_root: H256::zero(),
        }
    }

    #[test]
    fn stores_and_retrieves_block_and_receipts() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let store = ChainStore::new(storage);

        let block = sample_block(42);
        let block_hash = block.hash();
        let tx_hash = H256::from_slice(&[3u8; 32]).unwrap();
        let receipt = sample_receipt(tx_hash, block_hash, block.header.slot);

        store
            .store_block(&block, &[receipt.clone()])
            .expect("store block");

        let fetched_block = store
            .get_block_by_slot(block.header.slot)
            .unwrap()
            .expect("block by slot");
        assert_eq!(fetched_block.hash(), block_hash);

        let fetched_by_hash = store
            .get_block_by_hash(&block_hash)
            .unwrap()
            .expect("block by hash");
        assert_eq!(fetched_by_hash.header.slot, block.header.slot);

        let fetched_receipt = store.get_receipt(&tx_hash).unwrap().expect("receipt");
        assert_eq!(fetched_receipt.block_hash, block_hash);
        assert_eq!(fetched_receipt.slot, block.header.slot);
    }
}
