//! Optimistic Rollup Framework for Aether L2 Chains.
//!
//! L2 chains post state commitments to Aether L1. Fraud proofs
//! allow anyone to challenge invalid state transitions within
//! a challenge window.
//!
//! # Architecture
//! - **Sequencer**: Batches L2 transactions and posts commitments to L1
//! - **State Commitment**: Hash of L2 state root posted on L1
//! - **Fraud Proof**: Re-executes disputed transactions to prove invalidity
//! - **Challenge Window**: Period during which commitments can be challenged

pub mod fraud_proof;
pub mod state_commitment;

pub use fraud_proof::{FraudProof, FraudProofVerifier};
pub use state_commitment::{L2Batch, StateCommitment};
