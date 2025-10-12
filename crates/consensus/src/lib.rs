// ============================================================================
// AETHER CONSENSUS - Simplified consensus implementation
// ============================================================================
// PURPOSE: Provide a lightweight round-robin consensus used by the initial
// node prototype while the full VRF/HotStuff pipeline is being developed.
// ============================================================================

pub mod simple;

pub use simple::SimpleConsensus;
