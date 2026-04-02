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
        H256::from(<[u8; 32]>::from(hasher.finalize()))
    }

    /// Canonical message used for Ed25519 signing and verification.
    /// Includes slot, index, and payload hash to bind the signature
    /// to a specific shred without including the full payload.
    pub fn signing_message(&self) -> Vec<u8> {
        let mut msg = Vec::with_capacity(8 + 4 + 32);
        msg.extend_from_slice(&self.slot.to_le_bytes());
        msg.extend_from_slice(&self.index.to_le_bytes());
        msg.extend_from_slice(self.payload_hash.as_bytes());
        msg
    }

    /// Build the signing message from components (for use before Shred construction).
    pub fn build_signing_message(slot: Slot, index: u32, payload_hash: &H256) -> Vec<u8> {
        let mut msg = Vec::with_capacity(8 + 4 + 32);
        msg.extend_from_slice(&slot.to_le_bytes());
        msg.extend_from_slice(&index.to_le_bytes());
        msg.extend_from_slice(payload_hash.as_bytes());
        msg
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

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_h256() -> impl Strategy<Value = H256> {
        prop::array::uniform32(any::<u8>()).prop_map(|b| H256::from_slice(&b).unwrap())
    }

    fn arb_shred(payload: Vec<u8>, slot: u64, index: u32) -> Shred {
        Shred::new(
            ShredVariant::Data,
            slot,
            index,
            1,
            0,
            H256::zero(),
            payload,
            Signature::from_bytes(vec![0u8; 64]),
        )
    }

    proptest! {
        /// Payload hash is always deterministic: same payload always produces the same hash.
        #[test]
        fn hash_payload_deterministic(payload in prop::collection::vec(any::<u8>(), 0..256)) {
            let h1 = Shred::hash_payload(&payload);
            let h2 = Shred::hash_payload(&payload);
            prop_assert_eq!(h1, h2, "hash must be deterministic");
        }

        /// Two different payloads produce different hashes (collision resistance).
        #[test]
        fn hash_payload_collision_resistant(
            p1 in prop::collection::vec(any::<u8>(), 1..128),
            p2 in prop::collection::vec(any::<u8>(), 1..128),
        ) {
            prop_assume!(p1 != p2);
            let h1 = Shred::hash_payload(&p1);
            let h2 = Shred::hash_payload(&p2);
            prop_assert_ne!(h1, h2, "different payloads must produce different hashes");
        }

        /// Shred constructor always stores the correct payload_hash.
        #[test]
        fn shred_new_stores_correct_hash(
            payload in prop::collection::vec(any::<u8>(), 0..256),
            slot in any::<u64>(),
            index in any::<u32>(),
        ) {
            let shred = arb_shred(payload.clone(), slot, index);
            let expected = Shred::hash_payload(&payload);
            prop_assert_eq!(shred.payload_hash, expected,
                "shred.payload_hash must equal Shred::hash_payload(&payload)");
        }

        /// Signing message always has the correct fixed length (8 + 4 + 32 = 44 bytes).
        #[test]
        fn signing_message_has_fixed_length(
            payload in prop::collection::vec(any::<u8>(), 0..256),
            slot in any::<u64>(),
            index in any::<u32>(),
        ) {
            let shred = arb_shred(payload, slot, index);
            let msg = shred.signing_message();
            prop_assert_eq!(msg.len(), 44, "signing message must be 44 bytes");
        }

        /// signing_message and build_signing_message produce identical output.
        #[test]
        fn signing_message_matches_builder(
            payload in prop::collection::vec(any::<u8>(), 0..256),
            slot in any::<u64>(),
            index in any::<u32>(),
        ) {
            let shred = arb_shred(payload, slot, index);
            let via_method = shred.signing_message();
            let via_builder = Shred::build_signing_message(shred.slot, shred.index, &shred.payload_hash);
            prop_assert_eq!(via_method, via_builder,
                "signing_message() and build_signing_message() must agree");
        }

        /// Signing messages for different (slot, index, hash) triples are distinct.
        #[test]
        fn signing_message_unique_per_shred(
            slot1 in any::<u64>(), idx1 in any::<u32>(), h1 in arb_h256(),
            slot2 in any::<u64>(), idx2 in any::<u32>(), h2 in arb_h256(),
        ) {
            prop_assume!((slot1, idx1, h1) != (slot2, idx2, h2));
            let m1 = Shred::build_signing_message(slot1, idx1, &h1);
            let m2 = Shred::build_signing_message(slot2, idx2, &h2);
            prop_assert_ne!(m1, m2, "distinct shred parameters must yield distinct signing messages");
        }

        /// Shred serializes and deserializes to the same value.
        #[test]
        fn shred_bincode_roundtrip(
            payload in prop::collection::vec(any::<u8>(), 0..128),
            slot in any::<u64>(),
            index in any::<u32>(),
        ) {
            let shred = arb_shred(payload, slot, index);
            let encoded = bincode::serialize(&shred).expect("serialize");
            let decoded: Shred = bincode::deserialize(&encoded).expect("deserialize");
            prop_assert_eq!(shred, decoded, "bincode roundtrip must preserve shred");
        }

        /// Tampering with payload after construction invalidates the stored hash.
        #[test]
        fn tampered_payload_invalidates_hash(
            payload in prop::collection::vec(any::<u8>(), 1..128),
            tamper_byte in any::<u8>(),
            slot in any::<u64>(),
            index in any::<u32>(),
        ) {
            let mut shred = arb_shred(payload.clone(), slot, index);
            // Flip the first byte so payload definitely changes
            let original_first = shred.payload[0];
            let new_byte = tamper_byte.wrapping_add(1).max(original_first.wrapping_add(1));
            shred.payload[0] = if new_byte == original_first { original_first.wrapping_add(1) } else { new_byte };
            prop_assert_ne!(
                shred.payload_hash,
                Shred::hash_payload(&shred.payload),
                "stored hash must differ from tampered payload hash"
            );
        }
    }
}
