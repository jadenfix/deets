use aether_types::{Signature, Slot, H256};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ShredVariant {
    Data,
    Parity,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Shred {
    pub variant: ShredVariant,
    pub slot: Slot,
    pub index: u32,
    pub version: u16,
    pub fec_set_index: u32,
    pub block_id: H256,
    pub payload: Vec<u8>,
    pub signature: Signature,
    pub payload_hash: H256,
}

impl Shred {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        variant: ShredVariant,
        slot: Slot,
        index: u32,
        version: u16,
        fec_set_index: u32,
        block_id: H256,
        payload: Vec<u8>,
        signature: Signature,
    ) -> Self {
        let payload_hash = Self::hash_payload(&payload);
        Shred {
            variant,
            slot,
            index,
            version,
            fec_set_index,
            block_id,
            payload,
            signature,
            payload_hash,
        }
    }

    pub fn hash_payload(payload: &[u8]) -> H256 {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(payload);
        H256::from_slice(&hasher.finalize()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_hash() {
        let shred = Shred::new(
            ShredVariant::Data,
            1,
            0,
            1,
            0,
            H256::zero(),
            b"payload".to_vec(),
            Signature::from_bytes(vec![1, 2, 3]),
        );
        assert_eq!(shred.payload_hash, Shred::hash_payload(b"payload"));
    }
}
