use aether_types::{Address, H256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A fraud proof challenging an invalid L2 state transition.
///
/// The challenger re-executes the disputed transaction(s) and shows
/// that the claimed post-state root is incorrect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudProof {
    /// The batch being challenged.
    pub batch_id: u64,
    /// Index of the first invalid transaction in the batch.
    pub tx_index: u32,
    /// Pre-state root (should match batch's pre_state_root).
    pub pre_state_root: H256,
    /// Correct post-state root (computed by challenger).
    pub correct_post_state_root: H256,
    /// The batch's claimed (incorrect) post-state root.
    pub claimed_post_state_root: H256,
    /// Challenger's address.
    pub challenger: Address,
    /// Merkle proof of the pre-state at the disputed transaction.
    pub state_proof: Vec<u8>,
    /// The disputed transaction data (for re-execution).
    pub tx_data: Vec<u8>,
}

impl FraudProof {
    pub fn hash(&self) -> H256 {
        let bytes = bincode::serialize(self).unwrap_or_default();
        H256::from_slice(&Sha256::digest(&bytes)).unwrap()
    }
}

/// Verifies fraud proofs against state commitments.
pub struct FraudProofVerifier {
    /// Bond required to submit a fraud proof (prevents spam).
    pub required_bond: u128,
    /// Reward for successful challenge (% of sequencer's bond).
    pub challenger_reward_pct: u8,
}

/// Result of fraud proof verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FraudProofResult {
    /// Fraud proof is valid — state transition was incorrect.
    Valid {
        slashed_sequencer: Address,
        challenger_reward: u128,
    },
    /// Fraud proof is invalid — state transition was correct.
    Invalid { reason: String },
}

impl FraudProofVerifier {
    pub fn new(required_bond: u128, challenger_reward_pct: u8) -> Self {
        FraudProofVerifier {
            required_bond,
            challenger_reward_pct,
        }
    }

    /// Verify a fraud proof.
    ///
    /// In production, this would re-execute the transaction in a sandboxed
    /// WASM environment and compare the resulting state root.
    /// For now, we verify structural consistency.
    pub fn verify(
        &self,
        proof: &FraudProof,
        batch_pre_state_root: &H256,
        batch_post_state_root: &H256,
        sequencer: &Address,
        sequencer_bond: u128,
    ) -> FraudProofResult {
        // Pre-state root must match the batch
        if proof.pre_state_root != *batch_pre_state_root {
            return FraudProofResult::Invalid {
                reason: "pre-state root mismatch".into(),
            };
        }

        // Claimed post-state must match the batch's post-state
        if proof.claimed_post_state_root != *batch_post_state_root {
            return FraudProofResult::Invalid {
                reason: "claimed post-state doesn't match batch".into(),
            };
        }

        // The correct state root must differ from the claimed one
        // (otherwise there's no fraud)
        if proof.correct_post_state_root == proof.claimed_post_state_root {
            return FraudProofResult::Invalid {
                reason: "correct state root matches claimed — no fraud".into(),
            };
        }

        // In production: re-execute tx_data starting from pre_state_root
        // and verify that the result matches correct_post_state_root.
        // For now, we trust the challenger's computation.

        let reward =
            sequencer_bond * self.challenger_reward_pct as u128 / 100;

        FraudProofResult::Valid {
            slashed_sequencer: *sequencer,
            challenger_reward: reward,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fraud_proof() -> FraudProof {
        FraudProof {
            batch_id: 1,
            tx_index: 5,
            pre_state_root: H256::from_slice(&[1u8; 32]).unwrap(),
            correct_post_state_root: H256::from_slice(&[2u8; 32]).unwrap(),
            claimed_post_state_root: H256::from_slice(&[3u8; 32]).unwrap(),
            challenger: Address::from_slice(&[10u8; 20]).unwrap(),
            state_proof: vec![0u8; 32],
            tx_data: vec![1, 2, 3],
        }
    }

    #[test]
    fn test_valid_fraud_proof() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let proof = make_fraud_proof();
        let sequencer = Address::from_slice(&[1u8; 20]).unwrap();

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &sequencer,
            1_000_000,
        );

        match result {
            FraudProofResult::Valid {
                challenger_reward, ..
            } => {
                assert_eq!(challenger_reward, 500_000); // 50% of 1M
            }
            other => panic!("expected Valid, got {:?}", other),
        }
    }

    #[test]
    fn test_reject_pre_state_mismatch() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let proof = make_fraud_proof();
        let wrong_pre = H256::from_slice(&[99u8; 32]).unwrap();
        let sequencer = Address::from_slice(&[1u8; 20]).unwrap();

        let result = verifier.verify(
            &proof,
            &wrong_pre,
            &proof.claimed_post_state_root,
            &sequencer,
            1_000_000,
        );

        assert!(matches!(result, FraudProofResult::Invalid { .. }));
    }

    #[test]
    fn test_reject_no_fraud() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let mut proof = make_fraud_proof();
        // Make correct == claimed → no fraud
        proof.correct_post_state_root = proof.claimed_post_state_root;
        let sequencer = Address::from_slice(&[1u8; 20]).unwrap();

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &sequencer,
            1_000_000,
        );

        assert!(matches!(result, FraudProofResult::Invalid { .. }));
    }

    #[test]
    fn test_fraud_proof_hash_deterministic() {
        let proof = make_fraud_proof();
        assert_eq!(proof.hash(), proof.hash());
    }
}
