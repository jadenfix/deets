use aether_types::{Address, H256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A batch of L2 transactions posted to L1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2Batch {
    /// Unique batch identifier.
    pub batch_id: u64,
    /// L2 chain identifier.
    pub chain_id: u64,
    /// Sequencer that created this batch.
    pub sequencer: Address,
    /// L2 state root BEFORE this batch.
    pub pre_state_root: H256,
    /// L2 state root AFTER this batch.
    pub post_state_root: H256,
    /// Hashes of all L2 transactions in this batch.
    pub tx_hashes: Vec<H256>,
    /// L1 slot when this batch was posted.
    pub l1_slot: u64,
}

impl L2Batch {
    /// Hash this batch for commitment.
    pub fn hash(&self) -> H256 {
        let bytes = bincode::serialize(self).expect("L2Batch serialization infallible");
        H256::from(<[u8; 32]>::from(Sha256::digest(&bytes)))
    }
}

/// A state commitment posted on L1 by the sequencer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateCommitment {
    /// The batch this commitment covers.
    pub batch: L2Batch,
    /// Challenge window end slot (after which commitment is finalized).
    pub challenge_deadline: u64,
    /// Whether a fraud proof has been submitted.
    pub challenged: bool,
    /// Whether the commitment is finalized (past challenge window, unchallenged).
    pub finalized: bool,
}

/// Challenge window duration (in L1 slots).
/// 7 days = 7 * 24 * 3600 * 2 = 1,209,600 slots at 500ms.
pub const CHALLENGE_WINDOW_SLOTS: u64 = 1_209_600;

impl StateCommitment {
    pub fn new(batch: L2Batch, current_l1_slot: u64) -> Self {
        StateCommitment {
            challenge_deadline: current_l1_slot.saturating_add(CHALLENGE_WINDOW_SLOTS),
            batch,
            challenged: false,
            finalized: false,
        }
    }

    /// Check if the challenge window has passed.
    pub fn is_past_challenge_window(&self, current_l1_slot: u64) -> bool {
        current_l1_slot > self.challenge_deadline
    }

    /// Finalize this commitment (only if unchallenged and past deadline).
    pub fn try_finalize(&mut self, current_l1_slot: u64) -> Result<(), String> {
        if self.challenged {
            return Err("commitment has been challenged".into());
        }
        if !self.is_past_challenge_window(current_l1_slot) {
            return Err("challenge window not expired".into());
        }
        self.finalized = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_batch(id: u64) -> L2Batch {
        L2Batch {
            batch_id: id,
            chain_id: 1,
            sequencer: Address::from_slice(&[1u8; 20]).unwrap(),
            pre_state_root: H256::from_slice(&[id as u8; 32]).unwrap(),
            post_state_root: H256::from_slice(&[(id + 1) as u8; 32]).unwrap(),
            tx_hashes: vec![H256::zero()],
            l1_slot: 1000,
        }
    }

    #[test]
    fn test_batch_hash_deterministic() {
        let batch = make_batch(1);
        assert_eq!(batch.hash(), batch.hash());
    }

    #[test]
    fn test_commitment_challenge_window() {
        let commitment = StateCommitment::new(make_batch(1), 1000);
        assert!(!commitment.is_past_challenge_window(1000));
        assert!(!commitment.is_past_challenge_window(1000 + CHALLENGE_WINDOW_SLOTS));
        assert!(commitment.is_past_challenge_window(1000 + CHALLENGE_WINDOW_SLOTS + 1));
    }

    #[test]
    fn test_finalize_after_window() {
        let mut commitment = StateCommitment::new(make_batch(1), 1000);
        let after_window = 1000 + CHALLENGE_WINDOW_SLOTS + 1;

        assert!(commitment.try_finalize(1500).is_err()); // Too early
        assert!(commitment.try_finalize(after_window).is_ok());
        assert!(commitment.finalized);
    }

    #[test]
    fn challenge_deadline_saturates_near_max_slot() {
        // With bare addition, current_l1_slot near u64::MAX would overflow.
        // With saturating_add, deadline clamps to u64::MAX.
        let commitment = StateCommitment::new(make_batch(1), u64::MAX - 100);
        assert_eq!(commitment.challenge_deadline, u64::MAX);
        // Should not be past window at any reasonable slot
        assert!(!commitment.is_past_challenge_window(u64::MAX - 50));
    }

    #[test]
    fn test_cannot_finalize_challenged() {
        let mut commitment = StateCommitment::new(make_batch(1), 1000);
        commitment.challenged = true;
        let after = 1000 + CHALLENGE_WINDOW_SLOTS + 1;
        assert!(commitment.try_finalize(after).is_err());
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

    fn arb_batch(sequencer: Address, pre: H256, post: H256, id: u64, l1_slot: u64) -> L2Batch {
        L2Batch {
            batch_id: id,
            chain_id: 1,
            sequencer,
            pre_state_root: pre,
            post_state_root: post,
            tx_hashes: vec![],
            l1_slot,
        }
    }

    proptest! {
        /// L2Batch hash is deterministic.
        #[test]
        fn batch_hash_deterministic(
            id in any::<u64>(),
            sequencer in arb_address(),
            pre in arb_h256(),
            post in arb_h256(),
            l1_slot in any::<u64>(),
        ) {
            let batch = arb_batch(sequencer, pre, post, id, l1_slot);
            prop_assert_eq!(batch.hash(), batch.hash());
        }

        /// Different batches (different id) produce different hashes.
        #[test]
        fn different_batches_different_hashes(
            id1 in 0u64..1_000_000,
            id2 in 0u64..1_000_000,
            sequencer in arb_address(),
            pre in arb_h256(),
            post in arb_h256(),
        ) {
            prop_assume!(id1 != id2);
            let b1 = arb_batch(sequencer, pre, post, id1, 0);
            let b2 = arb_batch(sequencer, pre, post, id2, 0);
            prop_assert_ne!(b1.hash(), b2.hash(),
                "different batch IDs must produce different hashes");
        }

        /// Slot at or before deadline is not past the challenge window.
        #[test]
        fn slot_at_deadline_not_past_window(
            l1_slot in 0u64..1_000_000u64,
            delta in 0u64..=CHALLENGE_WINDOW_SLOTS,
        ) {
            let seq = Address::from_slice(&[1u8; 20]).unwrap();
            let batch = arb_batch(seq, H256::zero(), H256::zero(), 1, l1_slot);
            let commitment = StateCommitment::new(batch, l1_slot);
            let query_slot = l1_slot.saturating_add(delta);
            prop_assert!(
                !commitment.is_past_challenge_window(query_slot),
                "slot within window must not be past deadline"
            );
        }

        /// Slot strictly after deadline is always past the challenge window.
        #[test]
        fn slot_after_deadline_past_window(
            l1_slot in 0u64..500_000u64,
            extra in 1u64..100_000u64,
        ) {
            let seq = Address::from_slice(&[1u8; 20]).unwrap();
            let batch = arb_batch(seq, H256::zero(), H256::zero(), 1, l1_slot);
            let commitment = StateCommitment::new(batch, l1_slot);
            let after = commitment.challenge_deadline.saturating_add(extra);
            prop_assert!(
                commitment.is_past_challenge_window(after),
                "slot after deadline must be past challenge window"
            );
        }

        /// Finalizing before challenge window always fails.
        #[test]
        fn finalize_before_window_fails(
            l1_slot in 0u64..500_000u64,
            early_delta in 0u64..=CHALLENGE_WINDOW_SLOTS,
        ) {
            let seq = Address::from_slice(&[1u8; 20]).unwrap();
            let batch = arb_batch(seq, H256::zero(), H256::zero(), 1, l1_slot);
            let mut commitment = StateCommitment::new(batch, l1_slot);
            let early_slot = l1_slot.saturating_add(early_delta);
            let result = commitment.try_finalize(early_slot);
            prop_assert!(result.is_err(), "finalize before window must fail");
            prop_assert!(!commitment.finalized, "must not be finalized after early attempt");
        }

        /// Finalizing after challenge window (unchallenged) always succeeds.
        #[test]
        fn finalize_after_window_succeeds(
            l1_slot in 0u64..500_000u64,
            extra in 1u64..100_000u64,
        ) {
            let seq = Address::from_slice(&[1u8; 20]).unwrap();
            let batch = arb_batch(seq, H256::zero(), H256::zero(), 1, l1_slot);
            let mut commitment = StateCommitment::new(batch, l1_slot);
            let after = commitment.challenge_deadline.saturating_add(extra);
            let result = commitment.try_finalize(after);
            prop_assert!(result.is_ok(), "finalize after window must succeed: {:?}", result);
            prop_assert!(commitment.finalized, "commitment must be finalized");
        }

        /// Challenged commitment can never be finalized.
        #[test]
        fn challenged_commitment_never_finalizes(
            l1_slot in 0u64..500_000u64,
            extra in 1u64..100_000u64,
        ) {
            let seq = Address::from_slice(&[1u8; 20]).unwrap();
            let batch = arb_batch(seq, H256::zero(), H256::zero(), 1, l1_slot);
            let mut commitment = StateCommitment::new(batch, l1_slot);
            commitment.challenged = true;
            let after = commitment.challenge_deadline.saturating_add(extra);
            let result = commitment.try_finalize(after);
            prop_assert!(result.is_err(), "challenged commitment must not finalize");
            prop_assert!(!commitment.finalized, "challenged commitment must remain unfinalized");
        }

        /// challenge_deadline = l1_slot + CHALLENGE_WINDOW_SLOTS (invariant).
        #[test]
        fn challenge_deadline_invariant(l1_slot in 0u64..1_000_000u64) {
            let seq = Address::from_slice(&[1u8; 20]).unwrap();
            let batch = arb_batch(seq, H256::zero(), H256::zero(), 1, l1_slot);
            let commitment = StateCommitment::new(batch, l1_slot);
            prop_assert_eq!(
                commitment.challenge_deadline,
                l1_slot.saturating_add(CHALLENGE_WINDOW_SLOTS),
                "challenge_deadline must equal l1_slot + CHALLENGE_WINDOW_SLOTS"
            );
        }
    }
}
