//! Aether Light Client
//!
//! Verifies chain state without downloading full blocks.
//!
//! # How it works
//! 1. Download block headers only (not full transactions)
//! 2. Verify finality by checking BLS aggregate signatures on headers
//! 3. Query account/UTXO state via Merkle proofs against the state root
//!
//! # Security model
//! - Trusts the validator set (configured at initialization)
//! - Verifies 2/3 stake signed off on each finalized header
//! - Merkle proofs are self-verifying against the state root in the header

pub mod header_store;
pub mod state_query;
pub mod verifier;

pub use header_store::HeaderStore;
pub use state_query::{StateProof, StateQuery};
pub use verifier::LightClientVerifier;
