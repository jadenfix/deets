// ============================================================================
// AETHER TYPES - Canonical Type Definitions
// ============================================================================
// PURPOSE: Shared types used across all Aether components
//
// CORE TYPES:
// - H256: 32-byte hash
// - Address: Account address (20 bytes)
// - Signature: Cryptographic signature
// - Block, Transaction, UTxO, Account
// - Slot, Epoch
//
// All types implement:
// - Serialize/Deserialize (serde)
// - Clone, Debug
// - Consistent encoding (for hashing)
// ============================================================================

pub mod account;
pub mod block;
pub mod consensus;
pub mod primitives;
pub mod transaction;

pub use account::{Account, Utxo};
pub use block::{AggregatedVote, Block, BlockHeader, VrfProof};
pub use consensus::{EpochInfo, ValidatorInfo, Vote};
pub use primitives::{Address, Epoch, PublicKey, Signature, Slot, H160, H256};
pub use transaction::{Transaction, TransactionReceipt, TransactionStatus, UtxoId, UtxoOutput};
