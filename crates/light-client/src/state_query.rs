use aether_state_merkle::MerkleProof;
use aether_types::{Address, H256};
use anyhow::{bail, Result};

/// A state proof received from a full node.
#[derive(Debug, Clone)]
pub struct StateProof {
    /// The Merkle proof for this key.
    pub proof: MerkleProof,
    /// The raw value bytes (if the key exists).
    pub value: Option<Vec<u8>>,
}

/// Light client state query engine.
///
/// Verifies Merkle proofs against the finalized state root
/// to read account balances, UTXO status, etc. without
/// downloading the full state.
pub struct StateQuery {
    /// The trusted finalized state root.
    trusted_root: H256,
}

impl StateQuery {
    pub fn new(trusted_root: H256) -> Self {
        StateQuery { trusted_root }
    }

    /// Update the trusted root (after verifying a new finalized header).
    pub fn update_root(&mut self, new_root: H256) {
        self.trusted_root = new_root;
    }

    /// Verify a state proof for an account.
    ///
    /// Returns the value if the proof is valid, or an error if verification fails.
    pub fn verify_account(&self, address: &Address, proof: &StateProof) -> Result<Option<Vec<u8>>> {
        // Check the proof's claimed root matches our trusted root
        if proof.proof.root != self.trusted_root {
            bail!(
                "proof root {:?} does not match trusted root {:?}",
                proof.proof.root,
                self.trusted_root
            );
        }

        // Verify the Merkle proof
        if !proof.proof.verify() {
            bail!("Merkle proof verification failed for {:?}", address);
        }

        // Check the proof is for the right key
        if proof.proof.key != *address {
            bail!("proof key mismatch");
        }

        Ok(proof.value.clone())
    }

    /// Verify an inclusion proof (key exists with a specific value hash).
    pub fn verify_inclusion(
        &self,
        address: &Address,
        expected_value_hash: &H256,
        proof: &StateProof,
    ) -> Result<bool> {
        self.verify_account(address, proof)?;

        match &proof.proof.value_hash {
            Some(vh) => Ok(vh == expected_value_hash),
            None => Ok(false), // Key doesn't exist
        }
    }

    /// Verify an exclusion proof (key does NOT exist).
    pub fn verify_exclusion(&self, address: &Address, proof: &StateProof) -> Result<bool> {
        self.verify_account(address, proof)?;
        Ok(proof.proof.value_hash.is_none())
    }

    pub fn trusted_root(&self) -> H256 {
        self.trusted_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_state_merkle::SparseMerkleTree;

    #[test]
    fn test_verify_inclusion_proof() {
        let mut tree = SparseMerkleTree::new();
        let addr = Address::from_slice(&[1u8; 20]).unwrap();
        let value_hash = H256::from_slice(&[2u8; 32]).unwrap();

        tree.update(addr, value_hash);
        let proof = tree.prove(&addr);

        let query = StateQuery::new(tree.root());
        let state_proof = StateProof {
            proof,
            value: Some(b"account data".to_vec()),
        };

        let result = query.verify_account(&addr, &state_proof).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_verify_exclusion_proof() {
        let mut tree = SparseMerkleTree::new();
        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        tree.update(addr1, H256::from_slice(&[2u8; 32]).unwrap());

        // Prove absence of a different key
        let addr2 = Address::from_slice(&[3u8; 20]).unwrap();
        let proof = tree.prove(&addr2);

        let query = StateQuery::new(tree.root());
        let state_proof = StateProof { proof, value: None };

        let excluded = query.verify_exclusion(&addr2, &state_proof).unwrap();
        assert!(excluded);
    }

    #[test]
    fn test_reject_wrong_root() {
        let mut tree = SparseMerkleTree::new();
        let addr = Address::from_slice(&[1u8; 20]).unwrap();
        tree.update(addr, H256::from_slice(&[2u8; 32]).unwrap());
        let proof = tree.prove(&addr);

        // Use a different trusted root
        let wrong_root = H256::from_slice(&[99u8; 32]).unwrap();
        let query = StateQuery::new(wrong_root);
        let state_proof = StateProof {
            proof,
            value: Some(vec![]),
        };

        assert!(query.verify_account(&addr, &state_proof).is_err());
    }

    #[test]
    fn test_reject_tampered_proof() {
        let mut tree = SparseMerkleTree::new();
        let addr = Address::from_slice(&[1u8; 20]).unwrap();
        tree.update(addr, H256::from_slice(&[2u8; 32]).unwrap());
        let mut proof = tree.prove(&addr);

        // Tamper with a sibling
        if !proof.siblings.is_empty() {
            proof.siblings[0] = H256::from_slice(&[99u8; 32]).unwrap();
        }

        let query = StateQuery::new(tree.root());
        let state_proof = StateProof {
            proof,
            value: Some(vec![]),
        };

        assert!(query.verify_account(&addr, &state_proof).is_err());
    }
}
