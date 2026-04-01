// ============================================================================
// AETHER NODE - Main Orchestrator
// ============================================================================
// PURPOSE: Top-level node coordinator that wires together all subsystems
// ============================================================================

pub mod feature_gates;
pub mod fork_choice;
pub mod genesis;
pub mod hybrid_node;
pub mod network_handler;
pub mod node;
pub mod poh;
pub mod sync;

pub use feature_gates::FeatureGateRegistry;
pub use genesis::GenesisConfig;
pub use hybrid_node::{
    create_hybrid_consensus, create_hybrid_consensus_with_all_keys,
    create_hybrid_consensus_with_vrf_keys, validator_info_from_keypair, ValidatorKeypair,
};
pub use network_handler::{decode_network_event, NodeMessage, OutboundMessage};
pub use node::Node;
pub use poh::{PohMetrics, PohRecorder};
