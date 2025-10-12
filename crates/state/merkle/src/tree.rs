use crate::proof::MerkleProof;
use aether_types::{Address, H256};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct SparseMerkleTree {
    root: H256,
    leaves: HashMap<Address, H256>,
}

impl SparseMerkleTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, key: Address, value_hash: H256) {
        self.leaves.insert(key, value_hash);
        self.recompute_root();
    }

    pub fn delete(&mut self, key: &Address) {
        self.leaves.remove(key);
        self.recompute_root();
    }

    pub fn get(&self, key: &Address) -> Option<H256> {
        self.leaves.get(key).copied()
    }

    pub fn root(&self) -> H256 {
        self.root
    }

    fn recompute_root(&mut self) {
        if self.leaves.is_empty() {
            self.root = H256::zero();
            return;
        }

        let mut entries: Vec<_> = self.leaves.iter().collect();
        entries.sort_by_key(|(addr, _)| addr.as_bytes().to_vec());

        let mut hasher = Sha256::new();
        for (addr, value) in entries {
            hasher.update(addr.as_bytes());
            hasher.update(value.as_bytes());
        }

        self.root = H256::from_slice(&hasher.finalize()).unwrap();
    }

    pub fn prove(&self, key: &Address) -> MerkleProof {
        MerkleProof::new(*key, self.leaves.get(key).copied(), self.root)
    }
}

impl Default for SparseMerkleTree {
    fn default() -> Self {
        SparseMerkleTree {
            root: H256::zero(),
            leaves: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tree() {
        let tree = SparseMerkleTree::new();
        assert_eq!(tree.root(), H256::zero());
    }

    #[test]
    fn test_update_and_get() {
        let mut tree = SparseMerkleTree::new();
        let addr = Address::from_slice(&[1u8; 20]).unwrap();
        let value = H256::from_slice(&[2u8; 32]).unwrap();

        tree.update(addr, value);
        assert_eq!(tree.get(&addr), Some(value));
    }

    #[test]
    fn test_root_changes_on_update() {
        let mut tree = SparseMerkleTree::new();
        let root1 = tree.root();

        let addr = Address::from_slice(&[1u8; 20]).unwrap();
        let value = H256::from_slice(&[2u8; 32]).unwrap();
        tree.update(addr, value);

        let root2 = tree.root();
        assert_ne!(root1, root2);
    }
}
