use anyhow::{bail, Result};

use crate::shred::Shred;

pub fn validate_shred(shred: &Shred, current_slot: u64, max_slot_age: u64) -> Result<()> {
    if shred.payload_hash != Shred::hash_payload(&shred.payload) {
        bail!("payload hash mismatch");
    }

    if shred.signature.as_bytes().is_empty() {
        bail!("missing signature");
    }

    if shred.slot + max_slot_age < current_slot {
        bail!("stale shred");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shred::{Shred, ShredVariant};
    use aether_types::{Signature, H256};

    #[test]
    fn validates_fresh_shred() {
        let shred = Shred::new(
            ShredVariant::Data,
            10,
            0,
            1,
            0,
            H256::zero(),
            vec![1, 2, 3],
            Signature::from_bytes(vec![1]),
        );
        assert!(validate_shred(&shred, 12, 5).is_ok());
    }

    #[test]
    fn rejects_stale_shred() {
        let shred = Shred::new(
            ShredVariant::Data,
            1,
            0,
            1,
            0,
            H256::zero(),
            vec![1, 2, 3],
            Signature::from_bytes(vec![1]),
        );
        assert!(validate_shred(&shred, 20, 5).is_err());
    }
}
