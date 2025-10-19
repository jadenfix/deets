// ============================================================================
// AETHER LEDGER - eUTxO++ State Management
// ============================================================================
// PURPOSE: Hybrid UTxO + account model with Sparse Merkle commitment
// ============================================================================

pub mod chain_store;
pub mod state;

pub use chain_store::ChainStore;
pub use state::Ledger;
