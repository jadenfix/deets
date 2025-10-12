use aether_types::{Address, H256};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MerkleProof {
    pub key: Address,
    pub value_hash: Option<H256>,
    pub root: H256,
}

impl MerkleProof {
    pub fn new(key: Address, value_hash: Option<H256>, root: H256) -> Self {
        MerkleProof {
            key,
            value_hash,
            root,
        }
    }

    pub fn verify(&self) -> bool {
        self.value_hash.is_some() || self.root == H256::zero()
    }
}
