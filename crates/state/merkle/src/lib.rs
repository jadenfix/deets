// ============================================================================
// AETHER STATE MERKLE - Sparse Merkle Tree for State Commitment
// ============================================================================
// PURPOSE: Succinct cryptographic commitment to entire ledger state
// ============================================================================

pub mod proof;
pub mod tree;

pub use proof::MerkleProof;
pub use tree::SparseMerkleTree;
