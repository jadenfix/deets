// ============================================================================
// AETHER CONSENSUS - Full consensus implementation
// ============================================================================
// PURPOSE: Provides multiple consensus engines:
// - SimpleConsensus: Round-robin for testing
// - VRF-PoS: VRF-based leader election
// - HotStuff: BFT consensus with BLS aggregation
// - HybridConsensus: VRF + HotStuff + BLS (full Phase 1 integration)
// ============================================================================

use aether_crypto_vrf::VrfProof;
use aether_types::{Block, PublicKey, Slot, Vote, H256};
use anyhow::Result;

/// Unified interface for all consensus engines
pub trait ConsensusEngine: Send + Sync {
    /// Get current slot number
    fn current_slot(&self) -> Slot;

    /// Advance to next slot
    fn advance_slot(&mut self);

    /// Check if validator is leader for given slot
    fn is_leader(&self, slot: Slot, validator_pubkey: &PublicKey) -> bool;

    /// Validate a proposed block
    fn validate_block(&self, block: &Block) -> Result<()>;

    /// Add a vote for a block
    fn add_vote(&mut self, vote: Vote) -> Result<()>;

    /// Check if a slot has reached finality
    fn check_finality(&mut self, slot: Slot) -> bool;

    /// Get highest finalized slot
    fn finalized_slot(&self) -> Slot;

    /// Get total stake in network
    fn total_stake(&self) -> u128;

    /// Get VRF proof for leader eligibility (if supported)
    fn get_leader_proof(&self, _slot: Slot) -> Option<VrfProof> {
        None
    }

    /// Record a block's parent relationship (for 2-chain finality).
    fn record_block(&mut self, _block_hash: H256, _parent_hash: H256, _slot: Slot) {
        // Default no-op for engines that don't need parent tracking
    }

    /// Update epoch randomness from a finalized block's VRF output.
    fn update_epoch_randomness(&mut self, _vrf_output: &[u8; 32]) {
        // Default no-op for engines without VRF
    }

    /// Get individual validator's registered stake by address.
    fn validator_stake(&self, _address: &aether_types::Address) -> u128 {
        0
    }

    /// Check if the current round has timed out (pacemaker).
    fn is_timed_out(&self) -> bool {
        false
    }

    /// Handle a timeout — advance phase/slot to prevent deadlock.
    fn on_timeout(&mut self) {}
}

/// Check if `voted_stake` represents a 2/3 quorum of `total_stake`.
/// Uses checked arithmetic to avoid overflow when stake values are very large.
pub fn has_quorum(voted_stake: u128, total_stake: u128) -> bool {
    if total_stake == 0 {
        return false;
    }
    // voted_stake * 3 >= total_stake * 2
    // Use checked_mul to prevent overflow
    match (voted_stake.checked_mul(3), total_stake.checked_mul(2)) {
        (Some(lhs), Some(rhs)) => lhs >= rhs,
        // If voted_stake*3 overflows, it's definitely >= total_stake*2
        // (since voted_stake <= total_stake, total_stake*2 can't overflow if voted_stake*3 does...
        //  but to be safe, use division fallback)
        _ => {
            // Fallback: use division to avoid overflow
            // voted_stake >= total_stake * 2 / 3
            // This rounds down, so check with +1 for ceiling
            let threshold = total_stake / 3 * 2 + if total_stake % 3 > 0 { 1 } else { 0 };
            voted_stake >= threshold
        }
    }
}

pub mod hotstuff;
pub mod hybrid;
pub mod pacemaker;
pub mod slashing;
pub mod simple;
pub mod vrf_pos;

pub use hotstuff::{ConsensusAction, HotStuffConsensus, TimeoutCertificate, TimeoutVote};
pub use hybrid::HybridConsensus;
pub use pacemaker::Pacemaker;
pub use simple::SimpleConsensus;
pub use vrf_pos::VrfPosConsensus;
