use aether_crypto_primitives::Keypair;
use aether_da_erasure::ReedSolomonEncoder;
use aether_da_shreds::{shred::ShredVariant, Shred};
use aether_types::{Signature, Slot, H256};
use anyhow::Result;

pub struct TurbineBroadcaster {
    encoder: ReedSolomonEncoder,
    protocol_version: u16,
    /// Ed25519 keypair used to sign shreds, proving proposer authenticity.
    signing_key: Keypair,
}

impl TurbineBroadcaster {
    pub fn new(
        data_shards: usize,
        parity_shards: usize,
        protocol_version: u16,
        signing_key: Keypair,
    ) -> Result<Self> {
        Ok(TurbineBroadcaster {
            encoder: ReedSolomonEncoder::new(data_shards, parity_shards)?,
            protocol_version,
            signing_key,
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

            let payload_hash = Shred::hash_payload(&chunk);
            let msg = Shred::build_signing_message(slot, idx as u32, &payload_hash);
            let signature = Signature::from_bytes(self.signing_key.sign(&msg));

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

    /// Returns the public key bytes for this broadcaster's signing key.
    pub fn public_key(&self) -> Vec<u8> {
        self.signing_key.public_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_expected_number_of_shreds() {
        let key = Keypair::generate();
        let broadcaster = TurbineBroadcaster::new(3, 1, 1, key).unwrap();
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

    #[test]
    fn shred_signatures_are_valid_ed25519() {
        let key = Keypair::generate();
        let pubkey = key.public_key();
        let broadcaster = TurbineBroadcaster::new(2, 1, 1, key).unwrap();
        let shreds = broadcaster
            .make_shreds(42, H256::zero(), b"test payload")
            .unwrap();

        for shred in &shreds {
            let msg = shred.signing_message();
            aether_crypto_primitives::verify(&pubkey, &msg, shred.signature.as_bytes())
                .expect("shred signature must be valid Ed25519");
        }
    }

    #[test]
    fn shred_signatures_reject_wrong_key() {
        let key = Keypair::generate();
        let wrong_key = Keypair::generate();
        let broadcaster = TurbineBroadcaster::new(2, 1, 1, key).unwrap();
        let shreds = broadcaster
            .make_shreds(1, H256::zero(), b"data")
            .unwrap();

        let wrong_pubkey = wrong_key.public_key();
        for shred in &shreds {
            let msg = shred.signing_message();
            assert!(
                aether_crypto_primitives::verify(&wrong_pubkey, &msg, shred.signature.as_bytes())
                    .is_err(),
                "signature must not verify under wrong public key"
            );
        }
    }
}
