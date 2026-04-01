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
        // bincode::serialize on a valid struct cannot fail;
        // SHA256 always produces 32 bytes matching H256.
        let bytes = bincode::serialize(self).expect("FraudProof serialization infallible");
        H256::from_slice(&Sha256::digest(&bytes)).expect("SHA256 produces 32 bytes")
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
    /// Fraud proof passed structural checks but awaits re-execution
    /// to confirm the challenger's claimed post-state root.
    Pending {
        batch_id: u64,
        challenger: Address,
    },
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
    /// Performs structural checks (pre-state match, claimed-state match,
    /// roots differ, challenger posted bond, tx_data non-empty, state_proof
    /// non-empty). If all structural checks pass, returns `Pending` —
    /// the proof must be re-executed in a sandboxed WASM environment before
    /// slashing can proceed. This prevents an attacker from slashing any
    /// sequencer by simply fabricating a `correct_post_state_root`.
    pub fn verify(
        &self,
        proof: &FraudProof,
        batch_pre_state_root: &H256,
        batch_post_state_root: &H256,
        _sequencer: &Address,
        _sequencer_bond: u128,
        challenger_bond: u128,
    ) -> FraudProofResult {
        // Challenger must post required bond to prevent spam
        if challenger_bond < self.required_bond {
            return FraudProofResult::Invalid {
                reason: format!(
                    "challenger bond {} below required {}",
                    challenger_bond, self.required_bond
                ),
            };
        }

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
        if proof.correct_post_state_root == proof.claimed_post_state_root {
            return FraudProofResult::Invalid {
                reason: "correct state root matches claimed — no fraud".into(),
            };
        }

        // Transaction data must be provided for re-execution
        if proof.tx_data.is_empty() {
            return FraudProofResult::Invalid {
                reason: "empty tx_data — cannot re-execute".into(),
            };
        }

        // State proof must be provided for pre-state verification
        if proof.state_proof.is_empty() {
            return FraudProofResult::Invalid {
                reason: "empty state_proof — cannot verify pre-state".into(),
            };
        }

        // SECURITY: Do NOT return Valid here. The challenger's
        // correct_post_state_root is unverified. A sandboxed re-execution
        // of tx_data against pre_state_root must confirm it before slashing.
        FraudProofResult::Pending {
            batch_id: proof.batch_id,
            challenger: proof.challenger,
        }
    }

    /// Finalize a fraud proof after re-execution confirms the challenger's
    /// claimed post-state root is correct.
    pub fn finalize_after_reexecution(
        &self,
        proof: &FraudProof,
        reexecuted_post_state_root: &H256,
        sequencer: &Address,
        sequencer_bond: u128,
    ) -> FraudProofResult {
        if *reexecuted_post_state_root == proof.claimed_post_state_root {
            // Re-execution agrees with the sequencer — no fraud
            return FraudProofResult::Invalid {
                reason: "re-execution confirms sequencer's state root".into(),
            };
        }

        if *reexecuted_post_state_root != proof.correct_post_state_root {
            // Re-execution disagrees with BOTH parties
            return FraudProofResult::Invalid {
                reason: "re-execution matches neither sequencer nor challenger".into(),
            };
        }

        // Re-execution confirms fraud
        let reward = sequencer_bond.saturating_mul(self.challenger_reward_pct as u128) / 100;

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

    fn sequencer() -> Address {
        Address::from_slice(&[1u8; 20]).unwrap()
    }

    #[test]
    fn test_structural_check_returns_pending() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let proof = make_fraud_proof();

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &sequencer(),
            1_000_000,
            1_000_000, // challenger bond meets requirement
        );

        // Must NOT return Valid — only Pending until re-execution
        match result {
            FraudProofResult::Pending {
                batch_id,
                challenger,
            } => {
                assert_eq!(batch_id, 1);
                assert_eq!(challenger, proof.challenger);
            }
            other => panic!("expected Pending, got {:?}", other),
        }
    }

    #[test]
    fn test_finalize_after_reexecution_confirms_fraud() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let proof = make_fraud_proof();

        // Re-execution confirms the challenger's root
        let result = verifier.finalize_after_reexecution(
            &proof,
            &proof.correct_post_state_root,
            &sequencer(),
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
    fn test_finalize_reexecution_agrees_with_sequencer() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let proof = make_fraud_proof();

        // Re-execution agrees with the sequencer — challenger was wrong
        let result = verifier.finalize_after_reexecution(
            &proof,
            &proof.claimed_post_state_root,
            &sequencer(),
            1_000_000,
        );

        assert!(matches!(result, FraudProofResult::Invalid { .. }));
    }

    #[test]
    fn test_finalize_reexecution_matches_neither() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let proof = make_fraud_proof();
        let neither = H256::from_slice(&[99u8; 32]).unwrap();

        let result =
            verifier.finalize_after_reexecution(&proof, &neither, &sequencer(), 1_000_000);

        match result {
            FraudProofResult::Invalid { reason } => {
                assert!(reason.contains("neither"));
            }
            other => panic!("expected Invalid, got {:?}", other),
        }
    }

    #[test]
    fn test_reject_insufficient_challenger_bond() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let proof = make_fraud_proof();

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &sequencer(),
            1_000_000,
            999_999, // below required bond
        );

        match result {
            FraudProofResult::Invalid { reason } => {
                assert!(reason.contains("bond"));
            }
            other => panic!("expected Invalid for low bond, got {:?}", other),
        }
    }

    #[test]
    fn test_reject_pre_state_mismatch() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let proof = make_fraud_proof();
        let wrong_pre = H256::from_slice(&[99u8; 32]).unwrap();

        let result = verifier.verify(
            &proof,
            &wrong_pre,
            &proof.claimed_post_state_root,
            &sequencer(),
            1_000_000,
            1_000_000,
        );

        assert!(matches!(result, FraudProofResult::Invalid { .. }));
    }

    #[test]
    fn test_reject_no_fraud() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let mut proof = make_fraud_proof();
        proof.correct_post_state_root = proof.claimed_post_state_root;

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &sequencer(),
            1_000_000,
            1_000_000,
        );

        assert!(matches!(result, FraudProofResult::Invalid { .. }));
    }

    #[test]
    fn test_reject_empty_tx_data() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let mut proof = make_fraud_proof();
        proof.tx_data = vec![];

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &sequencer(),
            1_000_000,
            1_000_000,
        );

        match result {
            FraudProofResult::Invalid { reason } => assert!(reason.contains("tx_data")),
            other => panic!("expected Invalid, got {:?}", other),
        }
    }

    #[test]
    fn test_reject_empty_state_proof() {
        let verifier = FraudProofVerifier::new(1_000_000, 50);
        let mut proof = make_fraud_proof();
        proof.state_proof = vec![];

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &sequencer(),
            1_000_000,
            1_000_000,
        );

        match result {
            FraudProofResult::Invalid { reason } => assert!(reason.contains("state_proof")),
            other => panic!("expected Invalid, got {:?}", other),
        }
    }

    #[test]
    fn test_fraud_proof_hash_deterministic() {
        let proof = make_fraud_proof();
        assert_eq!(proof.hash(), proof.hash());
    }
}
