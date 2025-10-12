use aether-types::{H256, Address};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MerkleProof {
    pub key: Address,
    pub value_hash: Option<H256>,
    pub siblings: Vec<H256>,
}

impl MerkleProof {
    pub fn verify(&self, root: &H256) -> bool {
        if self.siblings.is_empty() {
            return self.value_hash.is_none();
        }

        let mut current = self.value_hash.unwrap_or(H256::zero());
        
        for sibling in &self.siblings {
            current = hash_pair(&current, sibling);
        }
        
        current == *root
    }
}

fn hash_pair(a: &H256, b: &H256) -> H256 {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(a.as_bytes());
    hasher.update(b.as_bytes());
    H256::from_slice(&hasher.finalize()).unwrap()
}

