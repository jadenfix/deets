// ============================================================================
// AETHER STATE STORAGE - RocksDB Persistence Layer
// ============================================================================
// PURPOSE: High-performance persistent storage for blockchain state
//
// DATABASE: RocksDB (LSM-tree based key-value store)
//
// COLUMN FAMILIES:
// - accounts: Address → Account data
// - utxos: UtxoId → Utxo data
// - merkle_nodes: NodeHash → Merkle node
// - blocks: BlockHash → Block data
// - receipts: TxHash → Receipt
// - metadata: Key → Value (state root, chain tip, etc.)
// ============================================================================

pub mod database;

pub use database::{Storage, StorageBatch, CF_ACCOUNTS, CF_UTXOS, CF_MERKLE, CF_BLOCKS, CF_RECEIPTS, CF_METADATA};

