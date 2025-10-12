use aether_da_erasure::ReedSolomonEncoder;
use aether_da_shreds::{shred::ShredVariant, Shred};
use aether_types::{Signature, Slot, H256};
use anyhow::Result;
use sha2::{Digest, Sha256};

pub struct TurbineBroadcaster {
    encoder: ReedSolomonEncoder,
    protocol_version: u16,
}

impl TurbineBroadcaster {
    pub fn new(data_shards: usize, parity_shards: usize, protocol_version: u16) -> Result<Self> {
        Ok(TurbineBroadcaster {
            encoder: ReedSolomonEncoder::new(data_shards, parity_shards)?,
            protocol_version,
        })
    }

    pub fn shard_count(&self) -> usize {
        self.encoder.data_shards + self.encoder.parity_shards
    }

    pub fn make_shreds(&self, slot: Slot, block_id: H256, payload: &[u8]) -> Result<Vec<Shred>> {
        let shards = self.encoder.encode(payload)?;
        let mut result = Vec::with_capacity(shards.len());

        for (idx, chunk) in shards.into_iter().enumerate() {
            let variant = if idx < self.encoder.data_shards {
                ShredVariant::Data
            } else {
                ShredVariant::Parity
            };

            let signature = self.sign_chunk(slot, idx as u32, &chunk);

            result.push(Shred::new(
                variant,
                slot,
                idx as u32,
                self.protocol_version,
                0,
                block_id,
                chunk,
                signature,
            ));
        }

        Ok(result)
    }

    fn sign_chunk(&self, slot: Slot, index: u32, chunk: &[u8]) -> Signature {
        let mut hasher = Sha256::new();
        hasher.update(slot.to_be_bytes());
        hasher.update(index.to_be_bytes());
        hasher.update(chunk);
        Signature::from_bytes(hasher.finalize().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_expected_number_of_shreds() {
        let broadcaster = TurbineBroadcaster::new(3, 1, 1).unwrap();
        let shreds = broadcaster
            .make_shreds(1, H256::zero(), b"block data")
            .unwrap();
        assert_eq!(shreds.len(), 4);
        assert!(matches!(shreds[0].variant, ShredVariant::Data));
        assert!(matches!(
            shreds.last().unwrap().variant,
            ShredVariant::Parity
        ));
    }
}
