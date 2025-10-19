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

    /// Create a vote for the given block hash, if this node is a validator.
    /// Implementations should sign the vote using the consensus-specific scheme
    /// (BLS for HybridConsensus, ed25519 for simple consensus) and return it so
    /// callers can broadcast/process it locally.
    fn create_vote(&self, _block_hash: H256) -> Result<Option<Vote>> {
        Ok(None)
    }
}

pub mod hotstuff;
pub mod hybrid;
pub mod simple;
pub mod vrf_pos;

pub use hotstuff::HotStuffConsensus;
pub use hybrid::HybridConsensus;
pub use simple::SimpleConsensus;
pub use vrf_pos::VrfPosConsensus;
