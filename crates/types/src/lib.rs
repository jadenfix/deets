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

pub mod primitives;
pub mod block;
pub mod transaction;
pub mod account;
pub mod consensus;

pub use primitives::{H256, H160, Address, Signature, PublicKey, Slot, Epoch};
pub use block::{Block, BlockHeader, VrfProof, AggregatedVote};
pub use transaction::{Transaction, TransactionReceipt, TransactionStatus, UtxoId, UtxoOutput};
pub use account::{Account, Utxo};
pub use consensus::{Vote, ValidatorInfo, EpochInfo};

