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

/// Trait for re-executing L2 transactions to verify fraud proofs.
///
/// Implementations bridge to the actual WASM VM or state transition function.
/// This indirection is required so the rollup crate does not take a hard
/// dependency on `aether-runtime` while still enforcing re-execution.
pub trait ReExecutor {
    /// Execute `tx_data` against the state identified by `pre_state_root`
    /// and return the resulting post-state root.
    ///
    /// Returns `Err` if execution fails (e.g. invalid tx encoding, VM fault).
    fn re_execute(&self, pre_state_root: &H256, tx_data: &[u8]) -> Result<H256, String>;
}

/// Verifies fraud proofs against state commitments.
pub struct FraudProofVerifier {
    /// Bond required to submit a fraud proof (prevents spam).
    pub required_bond: u128,
    /// Reward for successful challenge as a percentage of sequencer's bond (0-100).
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
    /// Create a new verifier.
    ///
    /// Returns an error if `challenger_reward_pct > 100` to prevent rewards
    /// that exceed the sequencer's bond.
    pub fn new(required_bond: u128, challenger_reward_pct: u8) -> Result<Self, String> {
        if challenger_reward_pct > 100 {
            return Err(format!(
                "challenger_reward_pct {} exceeds 100 — would pay out more than bond",
                challenger_reward_pct
            ));
        }
        Ok(FraudProofVerifier {
            required_bond,
            challenger_reward_pct,
        })
    }

    /// Verify a fraud proof by re-executing the disputed transaction.
    ///
    /// # Arguments
    /// - `proof` — the fraud proof submitted by the challenger.
    /// - `batch_pre_state_root` — the batch's recorded pre-state root (from chain).
    /// - `batch_post_state_root` — the batch's recorded post-state root (from chain).
    /// - `sequencer` — the sequencer who published the batch.
    /// - `sequencer_bond` — the sequencer's current bond balance.
    /// - `challenger_bond` — the bond posted by the challenger (must ≥ `required_bond`).
    /// - `executor` — re-execution engine used to compute the true post-state root.
    #[allow(clippy::too_many_arguments)]
    pub fn verify(
        &self,
        proof: &FraudProof,
        batch_pre_state_root: &H256,
        batch_post_state_root: &H256,
        sequencer: &Address,
        sequencer_bond: u128,
        challenger_bond: u128,
        executor: &dyn ReExecutor,
    ) -> FraudProofResult {
        // Challenger must have posted sufficient bond to prevent spam.
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
        // (otherwise there's no fraud)
        if proof.correct_post_state_root == proof.claimed_post_state_root {
            return FraudProofResult::Invalid {
                reason: "correct state root matches claimed — no fraud".into(),
            };
        }

        // Re-execute the disputed transaction from the pre-state and verify
        // that the result matches the challenger's claimed correct_post_state_root.
        // This is the critical check that prevents a challenger from fabricating
        // a fraud proof by supplying an arbitrary correct_post_state_root.
        let computed_post_state = match executor.re_execute(&proof.pre_state_root, &proof.tx_data)
        {
            Ok(root) => root,
            Err(reason) => {
                return FraudProofResult::Invalid {
                    reason: format!("re-execution failed: {}", reason),
                }
            }
        };

        if computed_post_state != proof.correct_post_state_root {
            return FraudProofResult::Invalid {
                reason: "re-executed post-state root does not match challenger's claim".into(),
            };
        }

        // Re-execution confirms the sequencer's post_state_root was wrong.
        let reward = sequencer_bond
            .saturating_mul(self.challenger_reward_pct as u128)
            / 100;

        FraudProofResult::Valid {
            slashed_sequencer: *sequencer,
            challenger_reward: reward,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> Address {
        Address::from_slice(&[byte; 20]).unwrap()
    }
    fn root(byte: u8) -> H256 {
        H256::from_slice(&[byte; 32]).unwrap()
    }

    fn make_fraud_proof() -> FraudProof {
        FraudProof {
            batch_id: 1,
            tx_index: 5,
            pre_state_root: root(1),
            correct_post_state_root: root(2),
            claimed_post_state_root: root(3),
            challenger: addr(10),
            state_proof: vec![0u8; 32],
            tx_data: vec![1, 2, 3],
        }
    }

    /// Minimal re-executor: returns a pre-programmed result.
    struct MockExecutor {
        result: Result<H256, String>,
    }
    impl ReExecutor for MockExecutor {
        fn re_execute(&self, _pre: &H256, _tx: &[u8]) -> Result<H256, String> {
            self.result.clone()
        }
    }

    #[test]
    fn test_new_rejects_reward_over_100() {
        assert!(FraudProofVerifier::new(1_000, 101).is_err());
        assert!(FraudProofVerifier::new(1_000, 100).is_ok());
    }

    #[test]
    fn test_valid_fraud_proof() {
        let verifier = FraudProofVerifier::new(1_000_000, 50).unwrap();
        let proof = make_fraud_proof();
        let executor = MockExecutor {
            result: Ok(proof.correct_post_state_root),
        };

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &addr(1),
            1_000_000,
            1_000_000,
            &executor,
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
    fn test_reject_insufficient_challenger_bond() {
        let verifier = FraudProofVerifier::new(1_000_000, 50).unwrap();
        let proof = make_fraud_proof();
        let executor = MockExecutor {
            result: Ok(proof.correct_post_state_root),
        };

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &addr(1),
            1_000_000,
            500_000, // below required_bond
            &executor,
        );

        assert!(matches!(result, FraudProofResult::Invalid { .. }));
    }

    #[test]
    fn test_reject_pre_state_mismatch() {
        let verifier = FraudProofVerifier::new(1_000_000, 50).unwrap();
        let proof = make_fraud_proof();
        let executor = MockExecutor {
            result: Ok(proof.correct_post_state_root),
        };

        let result = verifier.verify(
            &proof,
            &root(99),
            &proof.claimed_post_state_root,
            &addr(1),
            1_000_000,
            1_000_000,
            &executor,
        );

        assert!(matches!(result, FraudProofResult::Invalid { .. }));
    }

    #[test]
    fn test_reject_no_fraud() {
        let verifier = FraudProofVerifier::new(1_000_000, 50).unwrap();
        let mut proof = make_fraud_proof();
        proof.correct_post_state_root = proof.claimed_post_state_root;
        let executor = MockExecutor {
            result: Ok(proof.correct_post_state_root),
        };

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &addr(1),
            1_000_000,
            1_000_000,
            &executor,
        );

        assert!(matches!(result, FraudProofResult::Invalid { .. }));
    }

    #[test]
    fn test_reject_fabricated_correct_state_root() {
        // Attacker claims correct_post_state_root = root(2) but re-execution says root(9).
        // This is the core anti-fabrication check.
        let verifier = FraudProofVerifier::new(1_000_000, 50).unwrap();
        let proof = make_fraud_proof(); // correct_post_state_root = root(2)
        let executor = MockExecutor {
            result: Ok(root(9)), // re-execution disagrees with challenger
        };

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &addr(1),
            1_000_000,
            1_000_000,
            &executor,
        );

        assert!(
            matches!(result, FraudProofResult::Invalid { reason } if
                reason.contains("re-executed post-state root does not match challenger's claim"))
        );
    }

    #[test]
    fn test_reject_re_execution_failure() {
        let verifier = FraudProofVerifier::new(1_000_000, 50).unwrap();
        let proof = make_fraud_proof();
        let executor = MockExecutor {
            result: Err("vm fault: out of gas".into()),
        };

        let result = verifier.verify(
            &proof,
            &proof.pre_state_root,
            &proof.claimed_post_state_root,
            &addr(1),
            1_000_000,
            1_000_000,
            &executor,
        );

        assert!(matches!(result, FraudProofResult::Invalid { .. }));
    }

    #[test]
    fn test_fraud_proof_hash_deterministic() {
        let proof = make_fraud_proof();
        assert_eq!(proof.hash(), proof.hash());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_address() -> impl Strategy<Value = Address> {
        prop::array::uniform20(any::<u8>()).prop_map(|b| Address::from_slice(&b).unwrap())
    }

    fn arb_h256() -> impl Strategy<Value = H256> {
        prop::array::uniform32(any::<u8>()).prop_map(|b| H256::from_slice(&b).unwrap())
    }

    /// Mock re-executor that returns a fixed result.
    struct FixedExecutor(H256);
    impl ReExecutor for FixedExecutor {
        fn re_execute(&self, _pre: &H256, _tx: &[u8]) -> Result<H256, String> {
            Ok(self.0)
        }
    }

    /// Mock re-executor that always returns an error.
    struct FailingExecutor;
    impl ReExecutor for FailingExecutor {
        fn re_execute(&self, _pre: &H256, _tx: &[u8]) -> Result<H256, String> {
            Err("re-execution failed".into())
        }
    }

    proptest! {
        /// FraudProof hash is deterministic.
        #[test]
        fn fraud_proof_hash_deterministic(
            batch_id in any::<u64>(),
            tx_index in any::<u32>(),
            pre in arb_h256(),
            correct in arb_h256(),
            claimed in arb_h256(),
            challenger in arb_address(),
        ) {
            let proof = FraudProof {
                batch_id,
                tx_index,
                pre_state_root: pre,
                correct_post_state_root: correct,
                claimed_post_state_root: claimed,
                challenger,
                state_proof: vec![1, 2, 3],
                tx_data: vec![4, 5, 6],
            };
            prop_assert_eq!(proof.hash(), proof.hash());
        }

        /// FraudProofVerifier::new rejects reward_pct > 100.
        #[test]
        fn verifier_rejects_reward_above_100(pct in 101u8..=255u8) {
            let result = FraudProofVerifier::new(1_000_000, pct);
            prop_assert!(result.is_err(), "reward_pct > 100 must be rejected");
        }

        /// FraudProofVerifier::new accepts reward_pct in [0, 100].
        #[test]
        fn verifier_accepts_valid_reward_pct(pct in 0u8..=100u8) {
            let result = FraudProofVerifier::new(1_000_000, pct);
            prop_assert!(result.is_ok(), "reward_pct <= 100 must be accepted");
        }

        /// Insufficient challenger bond always produces Invalid.
        #[test]
        fn insufficient_bond_always_invalid(
            required in 1u128..1_000_000u128,
            posted in 0u128..999_999u128,
            pre in arb_h256(),
            correct in arb_h256(),
            claimed in arb_h256(),
            sequencer in arb_address(),
            challenger in arb_address(),
        ) {
            prop_assume!(posted < required);
            prop_assume!(correct != claimed);
            let verifier = FraudProofVerifier::new(required, 50).unwrap();
            let proof = FraudProof {
                batch_id: 1,
                tx_index: 0,
                pre_state_root: pre,
                correct_post_state_root: correct,
                claimed_post_state_root: claimed,
                challenger,
                state_proof: vec![],
                tx_data: vec![],
            };
            let executor = FixedExecutor(correct);
            let result = verifier.verify(&proof, &pre, &claimed, &sequencer, 1_000_000, posted, &executor);
            prop_assert!(matches!(result, FraudProofResult::Invalid { .. }),
                "insufficient bond must yield Invalid");
        }

        /// Pre-state root mismatch always produces Invalid.
        #[test]
        fn pre_state_mismatch_invalid(
            proof_pre in arb_h256(),
            batch_pre in arb_h256(),
            correct in arb_h256(),
            claimed in arb_h256(),
            sequencer in arb_address(),
            challenger in arb_address(),
        ) {
            prop_assume!(proof_pre != batch_pre);
            prop_assume!(correct != claimed);
            let verifier = FraudProofVerifier::new(0, 50).unwrap();
            let proof = FraudProof {
                batch_id: 1,
                tx_index: 0,
                pre_state_root: proof_pre,
                correct_post_state_root: correct,
                claimed_post_state_root: claimed,
                challenger,
                state_proof: vec![],
                tx_data: vec![],
            };
            let executor = FixedExecutor(correct);
            let result = verifier.verify(&proof, &batch_pre, &claimed, &sequencer, 1_000_000, 1_000_000, &executor);
            prop_assert!(matches!(result, FraudProofResult::Invalid { .. }),
                "pre-state mismatch must yield Invalid");
        }

        /// Correct == claimed (no fraud) always produces Invalid.
        #[test]
        fn no_fraud_always_invalid(
            pre in arb_h256(),
            state in arb_h256(),
            sequencer in arb_address(),
            challenger in arb_address(),
        ) {
            let verifier = FraudProofVerifier::new(0, 50).unwrap();
            let proof = FraudProof {
                batch_id: 1,
                tx_index: 0,
                pre_state_root: pre,
                correct_post_state_root: state, // same as claimed
                claimed_post_state_root: state,
                challenger,
                state_proof: vec![],
                tx_data: vec![],
            };
            let executor = FixedExecutor(state);
            let result = verifier.verify(&proof, &pre, &state, &sequencer, 1_000_000, 1_000_000, &executor);
            prop_assert!(matches!(result, FraudProofResult::Invalid { .. }),
                "correct == claimed must yield Invalid (no fraud)");
        }

        /// Re-execution failure always produces Invalid.
        #[test]
        fn re_execution_failure_invalid(
            pre in arb_h256(),
            correct in arb_h256(),
            claimed in arb_h256(),
            sequencer in arb_address(),
            challenger in arb_address(),
        ) {
            prop_assume!(correct != claimed);
            let verifier = FraudProofVerifier::new(0, 50).unwrap();
            let proof = FraudProof {
                batch_id: 1,
                tx_index: 0,
                pre_state_root: pre,
                correct_post_state_root: correct,
                claimed_post_state_root: claimed,
                challenger,
                state_proof: vec![],
                tx_data: vec![],
            };
            let result = verifier.verify(&proof, &pre, &claimed, &sequencer, 1_000_000, 1_000_000, &FailingExecutor);
            prop_assert!(matches!(result, FraudProofResult::Invalid { .. }),
                "re-execution failure must yield Invalid");
        }

        /// Fabricated correct_post_state_root (re-exec disagrees) always Invalid.
        #[test]
        fn fabricated_correct_root_invalid(
            pre in arb_h256(),
            correct in arb_h256(),
            claimed in arb_h256(),
            actual in arb_h256(),
            sequencer in arb_address(),
            challenger in arb_address(),
        ) {
            prop_assume!(correct != claimed);
            prop_assume!(actual != correct); // re-exec disagrees with challenger
            let verifier = FraudProofVerifier::new(0, 50).unwrap();
            let proof = FraudProof {
                batch_id: 1,
                tx_index: 0,
                pre_state_root: pre,
                correct_post_state_root: correct,
                claimed_post_state_root: claimed,
                challenger,
                state_proof: vec![],
                tx_data: vec![],
            };
            let executor = FixedExecutor(actual); // returns different from correct
            let result = verifier.verify(&proof, &pre, &claimed, &sequencer, 1_000_000, 1_000_000, &executor);
            prop_assert!(matches!(result, FraudProofResult::Invalid { .. }),
                "fabricated correct root must yield Invalid");
        }

        /// Valid fraud proof yields challenger_reward = sequencer_bond * pct / 100.
        #[test]
        fn valid_fraud_reward_correct(
            pre in arb_h256(),
            correct in arb_h256(),
            claimed in arb_h256(),
            sequencer in arb_address(),
            challenger in arb_address(),
            pct in 0u8..=100u8,
            bond in 0u128..1_000_000_000u128,
        ) {
            prop_assume!(correct != claimed);
            let verifier = FraudProofVerifier::new(0, pct).unwrap();
            let proof = FraudProof {
                batch_id: 1,
                tx_index: 0,
                pre_state_root: pre,
                correct_post_state_root: correct,
                claimed_post_state_root: claimed,
                challenger,
                state_proof: vec![],
                tx_data: vec![],
            };
            let executor = FixedExecutor(correct);
            let result = verifier.verify(&proof, &pre, &claimed, &sequencer, bond, 1_000_000_000, &executor);
            let expected_reward = bond.saturating_mul(pct as u128) / 100;
            match result {
                FraudProofResult::Valid { challenger_reward, slashed_sequencer } => {
                    prop_assert_eq!(challenger_reward, expected_reward,
                        "reward must be bond * pct / 100");
                    prop_assert_eq!(slashed_sequencer, sequencer,
                        "slashed sequencer must match");
                }
                FraudProofResult::Invalid { reason } => {
                    prop_assert!(false, "expected Valid, got Invalid: {}", reason);
                }
            }
        }
    }
}
