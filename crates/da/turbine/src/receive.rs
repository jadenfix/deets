use std::collections::{HashMap, VecDeque};

use aether_da_erasure::ReedSolomonDecoder;
use aether_da_shreds::Shred;
use aether_types::H256;
use anyhow::{bail, Result};

/// Maximum number of in-flight blocks to prevent memory exhaustion DoS.
const MAX_PENDING_BLOCKS: usize = 64;

/// Maximum aggregate bytes of pending shred payloads (128 MiB).
/// Bounds total memory a malicious peer can force the receiver to hold.
const MAX_PENDING_BYTES: usize = 128 * 1024 * 1024;

pub struct TurbineReceiver {
    decoder: ReedSolomonDecoder,
    pending: HashMap<H256, Vec<Option<Vec<u8>>>>,
    pending_order: VecDeque<H256>,
    pending_bytes: usize,
}

impl TurbineReceiver {
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self> {
        Ok(TurbineReceiver {
            decoder: ReedSolomonDecoder::new(data_shards, parity_shards)?,
            pending: HashMap::new(),
            pending_order: VecDeque::new(),
            pending_bytes: 0,
        })
    }

    fn block_bytes(shards: &[Option<Vec<u8>>]) -> usize {
        shards
            .iter()
            .map(|s| s.as_ref().map_or(0, |v| v.len()))
            .sum()
    }

    fn evict_oldest_pending(&mut self) {
        if let Some(block_id) = self.pending_order.pop_front() {
            if let Some(shards) = self.pending.remove(&block_id) {
                self.pending_bytes = self
                    .pending_bytes
                    .saturating_sub(Self::block_bytes(&shards));
            }
        }
    }

    fn remove_pending(&mut self, block_id: &H256) {
        if let Some(shards) = self.pending.remove(block_id) {
            self.pending_bytes = self
                .pending_bytes
                .saturating_sub(Self::block_bytes(&shards));
        }
        self.pending_order.retain(|queued| queued != block_id);
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

        let payload_len = shred.payload.len();

        if self.pending_bytes.saturating_add(payload_len) > MAX_PENDING_BYTES {
            bail!(
                "pending data limit exceeded ({} + {} > {})",
                self.pending_bytes,
                payload_len,
                MAX_PENDING_BYTES
            );
        }

        let is_new_block = !self.pending.contains_key(&shred.block_id);

        if is_new_block && self.pending.len() >= MAX_PENDING_BLOCKS {
            self.evict_oldest_pending();
        }

        let entry = self
            .pending
            .entry(shred.block_id)
            .or_insert_with(|| vec![None; total_shards]);

        if is_new_block {
            self.pending_order.push_back(shred.block_id);
        }

        if let Some(old) = entry[shred_idx].take() {
            self.pending_bytes = self.pending_bytes.saturating_sub(old.len());
        }
        entry[shred_idx] = Some(shred.payload.clone());
        self.pending_bytes = self.pending_bytes.saturating_add(payload_len);

        if entry.iter().filter(|chunk| chunk.is_some()).count() < data_shards {
            return Ok(None);
        }

        let recovered = self.decoder.decode(entry)?;
        self.remove_pending(&shred.block_id);
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

    #[test]
    fn evicts_oldest_pending_block_instead_of_rejecting_new_work() {
        let encoder = aether_da_erasure::ReedSolomonEncoder::new(2, 1).unwrap();
        let shards = encoder.encode(b"hello ").unwrap();

        let mut receiver = TurbineReceiver::new(2, 1).unwrap();
        for n in 0..MAX_PENDING_BLOCKS {
            let block_id = H256::from_slice(&[n as u8; 32]).unwrap();
            let shred = make_shred(block_id, 0, &shards[0]);
            assert!(receiver.ingest_shred(shred).unwrap().is_none());
        }

        let newest_block = H256::from_slice(&[0xF0; 32]).unwrap();
        let first = make_shred(newest_block, 0, &shards[0]);
        assert!(receiver.ingest_shred(first).unwrap().is_none());

        let second = make_shred(newest_block, 1, &shards[1]);
        let recovered = receiver.ingest_shred(second).unwrap().unwrap();
        assert_eq!(recovered, b"hello ");
    }

    #[test]
    fn rejects_shred_when_pending_bytes_exceeded() {
        let mut receiver = TurbineReceiver::new(2, 1).unwrap();
        // Fill pending_bytes to just under the limit
        receiver.pending_bytes = MAX_PENDING_BYTES - 10;

        let block_id = H256::zero();
        let large_payload = vec![0xAA; 64];
        let shred = make_shred(block_id, 0, &large_payload);
        let err = receiver.ingest_shred(shred).unwrap_err();
        assert!(
            err.to_string().contains("pending data limit exceeded"),
            "expected pending data limit error, got: {}",
            err
        );
    }

    #[test]
    fn pending_bytes_tracks_eviction() {
        let encoder = aether_da_erasure::ReedSolomonEncoder::new(2, 1).unwrap();
        let shards = encoder.encode(b"hello ").unwrap();

        let mut receiver = TurbineReceiver::new(2, 1).unwrap();
        let block_id = H256::zero();
        let s1 = make_shred(block_id, 0, &shards[0]);
        receiver.ingest_shred(s1).unwrap();
        assert!(receiver.pending_bytes > 0);

        let bytes_before = receiver.pending_bytes;
        receiver.evict_oldest_pending();
        assert_eq!(
            receiver.pending_bytes,
            bytes_before.saturating_sub(shards[0].len())
        );
    }

    #[test]
    fn pending_bytes_freed_on_successful_decode() {
        let encoder = aether_da_erasure::ReedSolomonEncoder::new(2, 1).unwrap();
        let data = b"hello ";
        let shards = encoder.encode(data).unwrap();

        let mut receiver = TurbineReceiver::new(2, 1).unwrap();
        let block_id = H256::zero();
        receiver
            .ingest_shred(make_shred(block_id, 0, &shards[0]))
            .unwrap();
        assert!(receiver.pending_bytes > 0);

        receiver
            .ingest_shred(make_shred(block_id, 1, &shards[1]))
            .unwrap();
        assert_eq!(
            receiver.pending_bytes, 0,
            "successful decode should free pending bytes"
        );
    }
}
