use crate::proof::MerkleProof;
use aether_types::{Address, H256};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct SparseMerkleTree {
    root: H256,
    leaves: HashMap<Address, H256>,
    dirty: bool, // Track if root needs recomputation
}

impl SparseMerkleTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, key: Address, value_hash: H256) {
        self.leaves.insert(key, value_hash);
        self.dirty = true; // Mark as dirty, defer recomputation
    }

    pub fn delete(&mut self, key: &Address) {
        self.leaves.remove(key);
        self.dirty = true; // Mark as dirty, defer recomputation
    }

    /// Batch update multiple keys at once (more efficient)
    pub fn batch_update(&mut self, updates: impl IntoIterator<Item = (Address, H256)>) {
        for (key, value_hash) in updates {
            self.leaves.insert(key, value_hash);
        }
        self.dirty = true;
    }

    pub fn get(&self, key: &Address) -> Option<H256> {
        self.leaves.get(key).copied()
    }

    pub fn root(&mut self) -> H256 {
        if self.dirty {
            self.recompute_root();
        }
        self.root
    }

    /// Force immediate root computation (for testing/debugging)
    pub fn compute_root(&mut self) {
        self.recompute_root();
    }

    fn recompute_root(&mut self) {
        if self.leaves.is_empty() {
            self.root = H256::zero();
            self.dirty = false;
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
        self.dirty = false;
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
            dirty: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tree() {
        let mut tree = SparseMerkleTree::new();
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

    #[test]
    fn test_lazy_root_computation() {
        let mut tree = SparseMerkleTree::new();

        // Update marks as dirty but doesn't compute
        let addr = Address::from_slice(&[1u8; 20]).unwrap();
        let value = H256::from_slice(&[2u8; 32]).unwrap();
        tree.update(addr, value);
        assert!(tree.dirty);

        // Root accessor triggers computation
        let _ = tree.root();
        assert!(!tree.dirty);
    }

    #[test]
    fn test_batch_update() {
        let mut tree1 = SparseMerkleTree::new();
        let mut tree2 = SparseMerkleTree::new();

        // Create test data
        let updates: Vec<_> = (0..100)
            .map(|i| {
                let mut addr_bytes = [0u8; 20];
                addr_bytes[0] = i as u8;
                let addr = Address::from_slice(&addr_bytes).unwrap();
                let value = H256::from_slice(&[i as u8; 32]).unwrap();
                (addr, value)
            })
            .collect();

        // Individual updates
        for (addr, value) in updates.clone() {
            tree1.update(addr, value);
        }

        // Batch update
        tree2.batch_update(updates);

        // Should produce same root
        assert_eq!(tree1.root(), tree2.root());
    }

    #[test]
    fn test_multiple_updates_before_root() {
        let mut tree = SparseMerkleTree::new();

        // Multiple updates
        for i in 0..10 {
            let mut addr_bytes = [0u8; 20];
            addr_bytes[0] = i;
            let addr = Address::from_slice(&addr_bytes).unwrap();
            let value = H256::from_slice(&[i; 32]).unwrap();
            tree.update(addr, value);
        }

        // Root computed only once
        let root = tree.root();
        assert!(!tree.dirty);

        // Second call returns cached value
        let root2 = tree.root();
        assert_eq!(root, root2);
    }

    #[test]
    fn test_delete_marks_dirty() {
        let mut tree = SparseMerkleTree::new();

        let addr = Address::from_slice(&[1u8; 20]).unwrap();
        let value = H256::from_slice(&[2u8; 32]).unwrap();
        tree.update(addr, value);
        let _ = tree.root(); // Compute
        assert!(!tree.dirty);

        // Delete should mark dirty
        tree.delete(&addr);
        assert!(tree.dirty);
    }
}
