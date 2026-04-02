// ============================================================================
// AETHER LEDGER - eUTxO++ State Management
// ============================================================================
// PURPOSE: Hybrid UTxO + account model with Sparse Merkle commitment
// ============================================================================

pub mod emission;
pub mod fee_market;
pub mod state;

#[cfg(test)]
mod proptest_tests;

pub use emission::EmissionSchedule;
pub use fee_market::FeeMarket;
pub use state::Ledger;
