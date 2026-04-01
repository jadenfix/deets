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
        H256::from_slice(&Sha256::digest(&bytes)).expect("SHA256 produces 32 bytes")
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
            challenge_deadline: current_l1_slot + CHALLENGE_WINDOW_SLOTS,
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
    fn test_cannot_finalize_challenged() {
        let mut commitment = StateCommitment::new(make_batch(1), 1000);
        commitment.challenged = true;
        let after = 1000 + CHALLENGE_WINDOW_SLOTS + 1;
        assert!(commitment.try_finalize(after).is_err());
    }
}
