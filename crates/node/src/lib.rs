// ============================================================================
// AETHER NODE - Main Orchestrator
// ============================================================================
// PURPOSE: Top-level node coordinator that wires together all subsystems
// ============================================================================

pub mod hybrid_node;
pub mod node;
pub mod poh;

pub use hybrid_node::{create_hybrid_consensus, validator_info_from_keypair, ValidatorKeypair};
pub use node::Node;
pub use poh::{PohMetrics, PohRecorder};
