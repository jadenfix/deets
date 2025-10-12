// ============================================================================
// AETHER STATE SNAPSHOTS - Fast Sync via State Checkpoints
// ============================================================================
// PURPOSE: Enable new nodes to sync without replaying entire chain history
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    SNAPSHOT SYSTEM                                │
// ├──────────────────────────────────────────────────────────────────┤
// │  Every Epoch End  →  Snapshot Generator  →  Frozen State Dump    │
// │         ↓                                      ↓                  │
// │  Upload to S3/IPFS  →  Announce via P2P  →  Bootstrap Nodes      │
// │         ↓                                      ↓                  │
// │  New Node  →  Download Snapshot  →  Verify State Root            │
// │         ↓                                      ↓                  │
// │  Import to RocksDB  →  Resume from Snapshot Height               │
// └──────────────────────────────────────────────────────────────────┘
//
// SNAPSHOT FORMAT:
// ```
// struct Snapshot:
//     height: u64
//     state_root: H256
//     epoch: u64
//     accounts: Vec<(Address, Account)>
//     utxos: Vec<(UtxoId, Utxo)>
//     merkle_tree: SparseMerkleTree
//     metadata: SnapshotMetadata
// ```
//
// GENERATION:
// ```
// fn generate_snapshot(height, storage):
//     // Export all accounts
//     accounts = []
//     for (addr, account) in storage.iter_accounts():
//         accounts.push((addr, account))
//
//     // Export all UTxOs
//     utxos = []
//     for (id, utxo) in storage.iter_utxos():
//         utxos.push((id, utxo))
//
//     // Export Merkle tree
//     merkle_tree = storage.export_merkle_tree()
//
//     // Get state root
//     state_root = storage.get_state_root()
//
//     snapshot = Snapshot {
//         height: height,
//         state_root: state_root,
//         epoch: height / EPOCH_SLOTS,
//         accounts: accounts,
//         utxos: utxos,
//         merkle_tree: merkle_tree,
//         metadata: build_metadata()
//     }
//
//     // Compress and write
//     compressed = zstd_compress(serialize(snapshot))
//     write_file(f"snapshot_{height}.bin", compressed)
//
//     return snapshot
// ```
//
// IMPORT:
// ```
// fn import_snapshot(snapshot_path, storage):
//     // Read and decompress
//     compressed = read_file(snapshot_path)
//     snapshot = deserialize(zstd_decompress(compressed))
//
//     // Verify state root
//     computed_root = compute_state_root(snapshot.accounts, snapshot.utxos)
//     if computed_root != snapshot.state_root:
//         return Err("invalid state root")
//
//     // Import to storage
//     batch = WriteBatch::new()
//
//     for (addr, account) in snapshot.accounts:
//         batch.put_account(addr, account)
//
//     for (id, utxo) in snapshot.utxos:
//         batch.put_utxo(id, utxo)
//
//     storage.import_merkle_tree(snapshot.merkle_tree)
//     storage.set_state_root(snapshot.state_root)
//     storage.set_chain_tip(snapshot.height)
//
//     storage.write(batch)
// ```
//
// SCHEDULE:
// - Generate every epoch (e.g., every 43200 slots = 6 hours)
// - Keep last 7 snapshots (1 week of history)
// - Archive old snapshots to cold storage
//
// PERFORMANCE:
// - 50GB state → compress to ~15GB snapshot
// - Generation: ~2 minutes
// - Import: ~5 minutes
// - Download: depends on bandwidth (1 Gbps = 2 minutes)
//
// OUTPUTS:
// - Snapshot files → S3/IPFS/local disk
// - Fast sync capability → Reduces bootstrap time from days to minutes
// ============================================================================

pub mod compression;
pub mod generator;
pub mod importer;

pub use generator::{decode_snapshot, generate_snapshot, SnapshotMetadata, StateSnapshot};
pub use importer::import_snapshot;
