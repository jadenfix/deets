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
