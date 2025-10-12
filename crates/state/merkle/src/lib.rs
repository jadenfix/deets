// ============================================================================
// AETHER STATE MERKLE - Sparse Merkle Tree for State Commitment
// ============================================================================
// PURPOSE: Succinct cryptographic commitment to entire ledger state
// ============================================================================

pub mod tree;
pub mod proof;

pub use tree::{SparseMerkleTree, MerkleProof};

