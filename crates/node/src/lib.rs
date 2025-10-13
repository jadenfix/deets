// ============================================================================
// AETHER NODE - Main Orchestrator
// ============================================================================
// PURPOSE: Top-level node coordinator that wires together all subsystems
// ============================================================================

pub mod node;
pub mod poh;

pub use node::Node;
pub use poh::{PohMetrics, PohRecorder};
