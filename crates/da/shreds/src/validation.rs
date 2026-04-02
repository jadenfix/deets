use anyhow::{bail, Result};

use crate::shred::Shred;

/// Validate a shred: payload hash, Ed25519 signature against proposer key, and freshness.
pub fn validate_shred(
    shred: &Shred,
    current_slot: u64,
    max_slot_age: u64,
    proposer_pubkey: &[u8],
) -> Result<()> {
    if shred.payload_hash != Shred::hash_payload(&shred.payload) {
        bail!("payload hash mismatch");
    }

    if shred.signature.as_bytes().is_empty() {
        bail!("missing signature");
    }

    // Verify Ed25519 signature against the proposer's public key
    let msg = shred.signing_message();
    aether_crypto_primitives::verify(proposer_pubkey, &msg, shred.signature.as_bytes())
        .map_err(|e| anyhow::anyhow!("invalid shred signature: {}", e))?;

    if shred.slot + max_slot_age < current_slot {
        bail!("stale shred");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shred::{Shred, ShredVariant};
    use aether_crypto_primitives::Keypair;
    use aether_types::{Signature, H256};

    fn make_signed_shred(key: &Keypair, slot: u64) -> Shred {
        let payload = vec![1, 2, 3];
        let payload_hash = Shred::hash_payload(&payload);
        let msg = Shred::build_signing_message(slot, 0, &payload_hash);
        let sig = Signature::from_bytes(key.sign(&msg));
        Shred::new(ShredVariant::Data, slot, 0, 1, 0, H256::zero(), payload, sig)
    }

    #[test]
    fn validates_correctly_signed_shred() {
        let key = Keypair::generate();
        let shred = make_signed_shred(&key, 10);
        assert!(validate_shred(&shred, 12, 5, &key.public_key()).is_ok());
    }

    #[test]
    fn rejects_wrong_proposer_key() {
        let key = Keypair::generate();
        let wrong_key = Keypair::generate();
        let shred = make_signed_shred(&key, 10);
        let err = validate_shred(&shred, 12, 5, &wrong_key.public_key()).unwrap_err();
        assert!(
            err.to_string().contains("invalid shred signature"),
            "expected signature error, got: {}",
            err
        );
    }

    #[test]
    fn rejects_forged_shred_with_fake_signature() {
        let key = Keypair::generate();
        let payload = vec![1, 2, 3];
        let fake_sig = Signature::from_bytes(vec![0u8; 64]);
        let shred = Shred::new(
            ShredVariant::Data,
            10,
            0,
            1,
            0,
            H256::zero(),
            payload,
            fake_sig,
        );
        assert!(validate_shred(&shred, 12, 5, &key.public_key()).is_err());
    }

    #[test]
    fn rejects_empty_signature() {
        let key = Keypair::generate();
        let payload = vec![1, 2, 3];
        let shred = Shred::new(
            ShredVariant::Data,
            10,
            0,
            1,
            0,
            H256::zero(),
            payload,
            Signature::from_bytes(vec![]),
        );
        let err = validate_shred(&shred, 12, 5, &key.public_key()).unwrap_err();
        assert!(err.to_string().contains("missing signature"));
    }

    #[test]
    fn rejects_stale_shred() {
        let key = Keypair::generate();
        let shred = make_signed_shred(&key, 1);
        assert!(validate_shred(&shred, 20, 5, &key.public_key()).is_err());
    }

    #[test]
    fn rejects_tampered_payload() {
        let key = Keypair::generate();
        let mut shred = make_signed_shred(&key, 10);
        // Tamper with payload after signing
        shred.payload = vec![9, 9, 9];
        // payload_hash won't match
        let err = validate_shred(&shred, 12, 5, &key.public_key()).unwrap_err();
        assert!(err.to_string().contains("payload hash mismatch"));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::shred::{Shred, ShredVariant};
    use aether_crypto_primitives::Keypair;
    use aether_types::{Signature, H256};
    use proptest::prelude::*;

    fn signed_shred(key: &Keypair, slot: u64, index: u32, payload: Vec<u8>) -> Shred {
        let payload_hash = Shred::hash_payload(&payload);
        let msg = Shred::build_signing_message(slot, index, &payload_hash);
        let sig = Signature::from_bytes(key.sign(&msg));
        Shred::new(ShredVariant::Data, slot, index, 1, 0, H256::zero(), payload, sig)
    }

    proptest! {
        /// A freshly signed shred always passes validation within the slot window.
        #[test]
        fn valid_shred_always_passes(
            slot in 100u64..10_000u64,
            max_age in 1u64..50u64,
            payload in prop::collection::vec(any::<u8>(), 1..64),
        ) {
            let key = Keypair::generate();
            let shred = signed_shred(&key, slot, 0, payload);
            // current_slot is within window: slot <= current_slot <= slot + max_age
            let current_slot = slot + max_age / 2;
            let result = validate_shred(&shred, current_slot, max_age, &key.public_key());
            prop_assert!(result.is_ok(), "valid shred must pass: {:?}", result);
        }

        /// A shred is always rejected if its slot is older than max_slot_age.
        #[test]
        fn stale_shred_always_rejected(
            slot in 1u64..1_000u64,
            excess in 1u64..100u64,
            max_age in 1u64..50u64,
            payload in prop::collection::vec(any::<u8>(), 1..32),
        ) {
            let key = Keypair::generate();
            let shred = signed_shred(&key, slot, 0, payload);
            // current_slot is beyond the freshness window
            let current_slot = slot.saturating_add(max_age).saturating_add(excess);
            let result = validate_shred(&shred, current_slot, max_age, &key.public_key());
            prop_assert!(result.is_err(), "stale shred must be rejected");
        }

        /// A shred signed by a wrong key always fails signature verification.
        #[test]
        fn wrong_key_always_rejected(
            slot in 1u64..1_000u64,
            payload in prop::collection::vec(any::<u8>(), 1..32),
        ) {
            let signer = Keypair::generate();
            let verifier = Keypair::generate();
            let shred = signed_shred(&signer, slot, 0, payload);
            let result = validate_shred(&shred, slot + 1, 10, &verifier.public_key());
            prop_assert!(result.is_err(), "wrong-key shred must fail");
        }

        /// An empty signature is always rejected.
        #[test]
        fn empty_signature_rejected(
            slot in 1u64..1_000u64,
            payload in prop::collection::vec(any::<u8>(), 1..32),
        ) {
            let key = Keypair::generate();
            let payload_hash = Shred::hash_payload(&payload);
            let shred = Shred::new(
                ShredVariant::Data, slot, 0, 1, 0, H256::zero(), payload,
                Signature::from_bytes(vec![]),
            );
            let _ = payload_hash; // used to ensure hash is computed above
            let result = validate_shred(&shred, slot + 1, 10, &key.public_key());
            prop_assert!(result.is_err(), "empty signature must be rejected");
        }

        /// Tampering with the payload after signing always causes validation to fail.
        #[test]
        fn tampered_payload_always_rejected(
            slot in 1u64..1_000u64,
            payload in prop::collection::vec(any::<u8>(), 2..64),
        ) {
            let key = Keypair::generate();
            let mut shred = signed_shred(&key, slot, 0, payload.clone());
            // Flip the last byte so payload differs from what was signed
            let last = shred.payload.len() - 1;
            shred.payload[last] = shred.payload[last].wrapping_add(1);
            let result = validate_shred(&shred, slot + 1, 10, &key.public_key());
            prop_assert!(result.is_err(), "tampered payload must be rejected");
        }
    }
}
