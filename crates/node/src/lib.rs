// ============================================================================
// AETHER NODE - Main Orchestrator
// ============================================================================
// PURPOSE: Top-level node coordinator that wires together all subsystems
// ============================================================================

pub mod node;
pub mod poh;
pub mod hybrid_node;

pub use node::Node;
pub use poh::{PohMetrics, PohRecorder};
pub use hybrid_node::{ValidatorKeypair, create_hybrid_consensus, validator_info_from_keypair};
