// ============================================================================
// AETHER CONSENSUS - VRF-PoS + HotStuff BFT
// ============================================================================
// PURPOSE: Leader election via VRF, block finality via BLS-aggregated votes
//
// Current: Simplified round-robin consensus for initial implementation
// TODO: Full VRF-PoS + HotStuff implementation
// ============================================================================

pub mod simple;
pub mod slashing;
pub mod vrf_pos;

pub use simple::SimpleConsensus;
pub use slashing::{detect_double_sign, verify_slash_proof, SlashProof, SlashType, Vote};
pub use vrf_pos::VrfPosConsensus;

