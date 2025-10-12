use anyhow::Result;
use bincode::{deserialize, serialize};

use crate::shred::Shred;

pub fn serialize_shred(shred: &Shred) -> Result<Vec<u8>> {
    Ok(serialize(shred)?)
}

pub fn deserialize_shred(bytes: &[u8]) -> Result<Shred> {
    Ok(deserialize(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shred::{Shred, ShredVariant};
    use aether_types::{Signature, H256};

    #[test]
    fn roundtrip() {
        let shred = Shred::new(
            ShredVariant::Data,
            1,
            0,
            1,
            0,
            H256::zero(),
            vec![1, 2, 3],
            Signature::from_bytes(vec![9, 9]),
        );

        let bytes = serialize_shred(&shred).unwrap();
        let decoded = deserialize_shred(&bytes).unwrap();
        assert_eq!(shred, decoded);
    }
}
