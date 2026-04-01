//! MEV Mitigation for Aether
//!
//! Implements commit-reveal transaction ordering to prevent front-running
//! and sandwich attacks by block proposers.
//!
//! # How it works
//! 1. **Commit phase**: Users submit encrypted transaction commitments
//!    (hash of encrypted tx). Proposer includes commitments in the block
//!    without seeing the actual transactions.
//! 2. **Reveal phase**: After the block with commitments is finalized,
//!    users reveal the actual transactions. They must match the commitments.
//! 3. **Execution**: Transactions are executed in commitment order
//!    (determined before content was known).
//!
//! This prevents the proposer from reordering, front-running, or
//! sandwiching transactions since they can't see tx content at ordering time.

pub mod commit_reveal;

pub use commit_reveal::{CommitRevealPool, RevealedTransaction, TransactionCommitment};
