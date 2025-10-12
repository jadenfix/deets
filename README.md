# Aether - AI-Credits Blockchain

A high-performance L1 blockchain combining Solana-style parallelism with Cardano-grade security for verifiable AI compute.

## Status: Foundation Complete ✅

**Working Components**:
- ✅ eUTxO++ hybrid ledger with Sparse Merkle Tree state commitments
- ✅ Ed25519 transaction signing and verification
- ✅ Fee-prioritized mempool with replace-by-fee
- ✅ RocksDB persistent storage with column families
- ✅ Simplified consensus (round-robin leader election, 2/3 finality)
- ✅ Block production and transaction execution
- ✅ Runnable single-node blockchain

**Architecture**:
- **Consensus**: Simplified PoS (full VRF-PoS + HotStuff planned)
- **Execution**: eUTxO++ with R/W set tracking for future parallel scheduling
- **Storage**: RocksDB with optimized configuration
- **State Commitment**: Sparse Merkle Tree for succinct state proofs
- **Tokens**: SWR (staking), AIC (AI credits) - programs to be implemented

## Quick Start

### Build

```bash
# Build all crates
cargo build --release

# Run tests
cargo test --all

# Check code
cargo clippy --all
```

### Run Single Node

```bash
# Run the node (will produce blocks for 10 seconds)
cargo run --release --bin aether-node

# Expected output:
# - Validator address
# - Transaction submission
# - Block production every 500ms
# - State root updates
```

### Development

```bash
# Build specific component
cargo build -p aether-ledger
cargo build -p aether-mempool

# Test specific component
cargo test -p aether-state-merkle
cargo test -p aether-consensus

# Run with logging
RUST_LOG=debug cargo run --bin aether-node
```

## Repository Structure

```
crates/
├── types/              # Core types (Block, Transaction, Account, etc.)
├── crypto/primitives/  # Ed25519, SHA-256, BLAKE3
├── state/
│   ├── storage/        # RocksDB wrapper
│   ├── merkle/         # Sparse Merkle Tree
│   └── snapshots/      # State snapshots (TODO)
├── ledger/             # eUTxO++ state management
├── mempool/            # Fee-prioritized transaction pool
├── consensus/          # Simplified PoS consensus
├── node/               # Node orchestrator + main.rs
└── [other crates...]   # Future: runtime, p2p, programs, etc.
```

## Implementation Progress

See [`IMPLEMENTATION_STATUS.md`](./IMPLEMENTATION_STATUS.md) for detailed breakdown.

**Summary**:
- **Foundation (Phases 0-1 core)**: 40% complete ✅
  - Types, crypto, storage, ledger, mempool, basic consensus: **DONE**
- **Full System (All 7 phases)**: ~15% complete
  - Remaining: Full consensus, P2P, WASM runtime, system programs, AI mesh

## Next Steps

1. **Full VRF-PoS Consensus** - Implement VRF leader election and BLS vote aggregation
2. **P2P Networking** - QUIC transport + Gossipsub for transaction/block propagation
3. **WASM Runtime** - Parallel execution engine with R/W set scheduling
4. **System Programs** - Staking, AIC token, Job Escrow, AMM
5. **AI Mesh** - TEE verifier, KZG challenges, VCR validation
6. **Multi-node Testing** - Deploy 4-node devnet
7. **RPC Server** - JSON-RPC for queries and transaction submission

## Testing

```bash
# Run all tests
cargo test --all --release

# Run specific test suites
cargo test -p aether-ledger -- --nocapture
cargo test -p aether-mempool -- --nocapture
cargo test -p aether-state-merkle -- --nocapture

# Property tests (future)
cargo test --features proptest
```

## Architecture Highlights

### eUTxO++ Hybrid Model
- Combines UTxO outputs (Bitcoin-style) with accounts (Ethereum-style)
- Transactions declare read/write sets for parallel execution
- Sparse Merkle Tree provides O(log n) state proofs

### Fee Market
- Priority queue by fee rate (fee / transaction size)
- Replace-by-fee with 10% premium
- 50k transaction mempool capacity

### Consensus
- Round-robin leader election (simplified for now)
- 2/3 stake finality threshold
- 500ms slot time
- Block production with automatic transaction execution

### Storage
- RocksDB with 6 column families (accounts, utxos, merkle_nodes, blocks, receipts, metadata)
- 256MB write buffer, LZ4 compression
- Atomic batch writes for consistency

## Scale Targets (Future)

- **L1**: 5-20k TPS with parallel execution
- **Finality**: <2s p95 (500ms slots)
- **Horizontal**: L2 app-chains, external DA
- **AI Mesh**: Millions of inference jobs/day

## Development Requirements

- Rust 1.75+
- 16GB+ RAM (for RocksDB)
- ~10GB disk space

## Contributing

See [`trm.md`](./trm.md) for the full technical roadmap spanning 7 phases.

## License

Apache 2.0

