use std::collections::HashMap;

use aether_da_erasure::ReedSolomonDecoder;
use aether_da_shreds::Shred;
use aether_types::H256;
use anyhow::{bail, Result};

/// Maximum number of in-flight blocks to prevent memory exhaustion DoS.
const MAX_PENDING_BLOCKS: usize = 64;

pub struct TurbineReceiver {
    decoder: ReedSolomonDecoder,
    pending: HashMap<H256, Vec<Option<Vec<u8>>>>,
}

impl TurbineReceiver {
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self> {
        Ok(TurbineReceiver {
            decoder: ReedSolomonDecoder::new(data_shards, parity_shards)?,
            pending: HashMap::new(),
        })
    }

    pub fn ingest_shred(&mut self, shred: Shred) -> Result<Option<Vec<u8>>> {
        let (data_shards, parity_shards) = self.decoder.shard_config();
        let total_shards = data_shards + parity_shards;
        let shred_idx = shred.index as usize;
        if shred_idx >= total_shards {
            bail!(
                "shred index {} exceeds shard count {}",
                shred.index,
                total_shards
            );
        }

        // Reject new blocks if we already have too many in-flight to prevent DoS
        if !self.pending.contains_key(&shred.block_id) && self.pending.len() >= MAX_PENDING_BLOCKS {
            bail!(
                "too many pending blocks ({} >= {}), dropping shred",
                self.pending.len(),
                MAX_PENDING_BLOCKS
            );
        }

        let entry = self
            .pending
            .entry(shred.block_id)
            .or_insert_with(|| vec![None; total_shards]);

        entry[shred_idx] = Some(shred.payload.clone());

        if entry.iter().filter(|chunk| chunk.is_some()).count() < data_shards {
            return Ok(None);
        }

        let recovered = self.decoder.decode(entry)?;
        self.pending.remove(&shred.block_id);
        Ok(Some(recovered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_da_shreds::{shred::ShredVariant, Shred};
    use aether_types::{Signature, H256};

    fn make_shred(block_id: H256, index: u32, payload: &[u8]) -> Shred {
        Shred::new(
            ShredVariant::Data,
            1,
            index,
            1,
            0,
            block_id,
            payload.to_vec(),
            Signature::from_bytes(vec![1, 2, 3]),
        )
    }

    #[test]
    fn reconstructs_when_enough_shreds() {
        // Use the encoder to produce properly length-prefixed shards
        let encoder = aether_da_erasure::ReedSolomonEncoder::new(2, 1).unwrap();
        let data = b"hello ";
        let shards = encoder.encode(data).unwrap();

        let mut receiver = TurbineReceiver::new(2, 1).unwrap();
        let block_id = H256::zero();
        let s1 = make_shred(block_id, 0, &shards[0]);
        let s2 = make_shred(block_id, 1, &shards[1]);

        assert!(receiver.ingest_shred(s1).unwrap().is_none());
        let recovered = receiver.ingest_shred(s2).unwrap().unwrap();
        assert_eq!(recovered, data);
    }
}
