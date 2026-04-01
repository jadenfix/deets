use aether_types::{Address, H256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A Merkle inclusion/exclusion proof for a key in the Sparse Merkle Tree.
///
/// Contains the sibling hashes along the path from the leaf to the root.
/// Can be verified independently without access to the tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MerkleProof {
    /// The key (address) this proof is for.
    pub key: Address,
    /// The value hash at this key, or None if key is absent.
    pub value_hash: Option<H256>,
    /// Expected root hash.
    pub root: H256,
    /// Sibling hashes from leaf to root (160 entries for Address-based keys).
    pub siblings: Vec<H256>,
}

impl MerkleProof {
    pub fn new(key: Address, value_hash: Option<H256>, root: H256, siblings: Vec<H256>) -> Self {
        MerkleProof {
            key,
            value_hash,
            root,
            siblings,
        }
    }

    /// Verify this proof against the claimed root.
    ///
    /// Siblings are in leaf-to-root order: siblings[0] is the deepest sibling
    /// (at the leaf level, corresponding to the last bit of the key).
    /// We walk from the leaf upward to reconstruct the root.
    pub fn verify(&self) -> bool {
        let key_bits = address_to_bits(&self.key);
        let depth = key_bits.len();

        if self.siblings.len() != depth {
            return false;
        }

        // Start with the leaf hash
        let mut current = match &self.value_hash {
            Some(vh) => leaf_hash(&self.key, vh),
            None => {
                use sha2::{Digest, Sha256};
                let mut h = Sha256::new();
                h.update([0x00]); // Leaf domain separator with no key/value
                H256::from_slice(&h.finalize()).unwrap()
            }
        };

        // Walk up: siblings[0] pairs with key_bits[depth-1] (deepest bit),
        // siblings[1] pairs with key_bits[depth-2], etc.
        for (i, sibling) in self.siblings.iter().enumerate() {
            let bit_index = depth - 1 - i;
            if key_bits[bit_index] {
                current = internal_hash(sibling, &current);
            } else {
                current = internal_hash(&current, sibling);
            }
        }

        current == self.root
    }
}

/// Hash two children to produce a parent node hash.
pub(crate) fn internal_hash(left: &H256, right: &H256) -> H256 {
    let mut hasher = Sha256::new();
    hasher.update([0x01]); // Internal node prefix
    hasher.update(left.as_bytes());
    hasher.update(right.as_bytes());
    H256::from_slice(&hasher.finalize()).unwrap()
}

/// Hash a leaf node (key + value).
pub(crate) fn leaf_hash(key: &Address, value_hash: &H256) -> H256 {
    let mut hasher = Sha256::new();
    hasher.update([0x00]); // Leaf node prefix
    hasher.update(key.as_bytes());
    hasher.update(value_hash.as_bytes());
    H256::from_slice(&hasher.finalize()).unwrap()
}

/// Convert an Address (20 bytes = 160 bits) to a bit path.
/// Bit 0 is the most significant bit of byte 0.
pub(crate) fn address_to_bits(addr: &Address) -> Vec<bool> {
    let bytes = addr.as_bytes();
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for byte in bytes {
        for i in (0..8).rev() {
            bits.push((byte >> i) & 1 == 1);
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_to_bits_length() {
        let addr = Address::from_slice(&[0u8; 20]).unwrap();
        let bits = address_to_bits(&addr);
        assert_eq!(bits.len(), 160);
    }

    #[test]
    fn test_address_to_bits_values() {
        let mut bytes = [0u8; 20];
        bytes[0] = 0b10110000;
        let addr = Address::from_slice(&bytes).unwrap();
        let bits = address_to_bits(&addr);
        assert!(bits[0]); // 1
        assert!(!bits[1]); // 0
        assert!(bits[2]); // 1
        assert!(bits[3]); // 1
    }

    #[test]
    fn test_leaf_hash_deterministic() {
        let addr = Address::from_slice(&[1u8; 20]).unwrap();
        let value = H256::from_slice(&[2u8; 32]).unwrap();
        let h1 = leaf_hash(&addr, &value);
        let h2 = leaf_hash(&addr, &value);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_internal_hash_deterministic() {
        let left = H256::from_slice(&[1u8; 32]).unwrap();
        let right = H256::from_slice(&[2u8; 32]).unwrap();
        let h1 = internal_hash(&left, &right);
        let h2 = internal_hash(&left, &right);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_internal_hash_order_matters() {
        let left = H256::from_slice(&[1u8; 32]).unwrap();
        let right = H256::from_slice(&[2u8; 32]).unwrap();
        let h1 = internal_hash(&left, &right);
        let h2 = internal_hash(&right, &left);
        assert_ne!(h1, h2);
    }
}
