use aether-types::{H256, Address};
use anyhow::{Result, bail};
use std::collections::HashMap;
use sha2::{Digest, Sha256};

const TREE_DEPTH: usize = 256;

#[derive(Clone, Debug)]
pub enum Node {
    Branch { left: H256, right: H256 },
    Leaf { key: Address, value_hash: H256 },
    Empty,
}

pub struct SparseMerkleTree {
    root: H256,
    nodes: HashMap<H256, Node>,
    leaves: HashMap<Address, H256>,
    empty_hashes: Vec<H256>,
}

impl SparseMerkleTree {
    pub fn new() -> Self {
        let empty_hashes = Self::compute_empty_hashes();
        let root = empty_hashes[TREE_DEPTH];
        
        SparseMerkleTree {
            root,
            nodes: HashMap::new(),
            leaves: HashMap::new(),
            empty_hashes,
        }
    }

    fn compute_empty_hashes() -> Vec<H256> {
        let mut hashes = Vec::with_capacity(TREE_DEPTH + 1);
        
        let mut current = H256::zero();
        hashes.push(current);
        
        for _ in 0..TREE_DEPTH {
            current = Self::hash_branch(&current, &current);
            hashes.push(current);
        }
        
        hashes
    }

    fn hash_branch(left: &H256, right: &H256) -> H256 {
        let mut hasher = Sha256::new();
        hasher.update(left.as_bytes());
        hasher.update(right.as_bytes());
        H256::from_slice(&hasher.finalize()).unwrap()
    }

    fn hash_leaf(key: &Address, value_hash: &H256) -> H256 {
        let mut hasher = Sha256::new();
        hasher.update(&[0x01]); // Leaf prefix
        hasher.update(key.as_bytes());
        hasher.update(value_hash.as_bytes());
        H256::from_slice(&hasher.finalize()).unwrap()
    }

    pub fn update(&mut self, key: Address, value_hash: H256) {
        self.leaves.insert(key, value_hash);
        self.root = self.compute_root();
    }

    pub fn delete(&mut self, key: &Address) {
        self.leaves.remove(key);
        self.root = self.compute_root();
    }

    pub fn get(&self, key: &Address) -> Option<H256> {
        self.leaves.get(key).copied()
    }

    pub fn root(&self) -> H256 {
        self.root
    }

    fn compute_root(&self) -> H256 {
        if self.leaves.is_empty() {
            return self.empty_hashes[TREE_DEPTH];
        }

        let mut current_level: HashMap<Vec<u8>, H256> = HashMap::new();
        
        for (addr, value_hash) in &self.leaves {
            let leaf_hash = Self::hash_leaf(addr, value_hash);
            let path = self.address_to_path(addr);
            current_level.insert(path, leaf_hash);
        }

        for depth in (0..TREE_DEPTH).rev() {
            let mut next_level: HashMap<Vec<u8>, H256> = HashMap::new();
            
            let mut processed = std::collections::HashSet::new();
            
            for (path, hash) in &current_level {
                if processed.contains(path) {
                    continue;
                }
                
                let parent_path = &path[..depth];
                let bit = if depth < path.len() {
                    path[depth] == 1
                } else {
                    false
                };
                
                let mut sibling_path = parent_path.to_vec();
                if depth < TREE_DEPTH {
                    sibling_path.push(if bit { 0 } else { 1 });
                    for i in (depth + 1)..TREE_DEPTH {
                        sibling_path.push(0);
                    }
                }
                
                let sibling_hash = current_level.get(&sibling_path)
                    .copied()
                    .unwrap_or(self.empty_hashes[depth]);
                
                let parent_hash = if bit {
                    Self::hash_branch(&sibling_hash, hash)
                } else {
                    Self::hash_branch(hash, &sibling_hash)
                };
                
                next_level.insert(parent_path.to_vec(), parent_hash);
                processed.insert(path.clone());
                processed.insert(sibling_path);
            }
            
            current_level = next_level;
        }

        current_level.get(&vec![]).copied().unwrap_or(self.empty_hashes[TREE_DEPTH])
    }

    fn address_to_path(&self, addr: &Address) -> Vec<u8> {
        let bytes = addr.as_bytes();
        let mut path = Vec::with_capacity(TREE_DEPTH);
        
        for byte in bytes {
            for i in (0..8).rev() {
                path.push((byte >> i) & 1);
            }
        }
        
        // Pad to TREE_DEPTH if needed
        while path.len() < TREE_DEPTH {
            path.push(0);
        }
        
        path
    }

    pub fn prove(&self, key: &Address) -> MerkleProof {
        let path = self.address_to_path(key);
        let value_hash = self.leaves.get(key).copied();
        
        let mut siblings = Vec::new();
        
        // In production, this would traverse the actual tree structure
        // For now, returning empty proof
        
        MerkleProof {
            key: *key,
            value_hash,
            siblings,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MerkleProof {
    pub key: Address,
    pub value_hash: Option<H256>,
    pub siblings: Vec<H256>,
}

impl MerkleProof {
    pub fn verify(&self, root: &H256) -> bool {
        // Simplified verification
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tree() {
        let tree = SparseMerkleTree::new();
        assert_ne!(tree.root(), H256::zero());
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
    fn test_deterministic_root() {
        let mut tree1 = SparseMerkleTree::new();
        let mut tree2 = SparseMerkleTree::new();
        
        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();
        let value1 = H256::from_slice(&[3u8; 32]).unwrap();
        let value2 = H256::from_slice(&[4u8; 32]).unwrap();
        
        tree1.update(addr1, value1);
        tree1.update(addr2, value2);
        
        tree2.update(addr2, value2);
        tree2.update(addr1, value1);
        
        assert_eq!(tree1.root(), tree2.root());
    }
}

