# Aether Blockchain - Implementation Roadmap

## Progress Overview

**Current Status**: Phase 1 - Core Ledger & Consensus (In Progress)

### Completed ✅

#### Phase 0: Foundation
- [x] Repository structure
- [x] Cargo workspace setup
- [x] Core type system (H256, Address, Signature, etc.)
- [x] eUTxO++ transaction model
- [x] Conflict detection algorithm
- [x] Sparse Merkle tree
- [x] RocksDB storage layer
- [x] Mempool with fee market
- [x] Basic consensus framework
- [x] Block production pipeline
- [x] Receipt system

#### Phase 1 Progress
- [x] Ed25519 signature verification infrastructure
- [x] Transaction sender_pubkey field
- [x] JSON-RPC server (8 methods)
  - aeth_sendRawTransaction
  - aeth_getBlockByNumber
  - aeth_getBlockByHash
  - aeth_getTransactionReceipt
  - aeth_getStateRoot
  - aeth_getAccount
  - aeth_getSlotNumber
  - aeth_getFinalizedSlot

### In Progress 🔄

#### Phase 1: Core Ledger & Consensus (Weeks 2-8)
- [🔄] ECVRF leader election
- [ ] BLS12-381 vote aggregation
- [ ] HotStuff 2-chain BFT
- [ ] WASM runtime (Wasmtime)
- [ ] Parallel scheduler
- [ ] Basic P2P networking

### Pending 📋

#### Phase 2: Economics & System Programs (Weeks 8-12)
- [ ] Staking program
- [ ] Governance program
- [ ] AMM DEX (constant product)
- [ ] AIC token logic
- [ ] Job escrow contract
- [ ] Reputation oracle
- [ ] Fee market enhancements
- [ ] State rent

#### Phase 3: AI Mesh & Verifiable Compute (Weeks 12-20)
- [ ] Deterministic inference builds
- [ ] SEV-SNP/TDX attestation
- [ ] KZG commitments
- [ ] VCR (Verifiable Compute Receipt)
- [ ] Challenge protocol
- [ ] Redundant quorum fallback
- [ ] Job router
- [ ] Provider reputation system

#### Phase 4: Networking, DA & Performance (Weeks 20-28)
- [ ] Turbine block propagation
- [ ] Reed-Solomon erasure coding
- [ ] Shred generation and reconstruction
- [ ] QUIC transport
- [ ] Libp2p gossipsub
- [ ] GPU batch signature verification
- [ ] BLS multiexponentiation
- [ ] PoH-like local sequencing
- [ ] RocksDB tuning

#### Phase 5: SRE, Observability, Ops (Weeks 28-36)
- [ ] OpenTelemetry tracing
- [ ] Prometheus metrics
- [ ] Grafana dashboards
- [ ] Alert rules
- [ ] Terraform modules (AWS/GCP/Azure)
- [ ] Helm charts
- [ ] Runbooks
- [ ] gRPC Firehose for indexers
- [ ] State sync protocol

#### Phase 6: Security & Formal Methods (Weeks 36-44)
- [ ] Threat modeling (STRIDE/LINDDUN)
- [ ] External audits
- [ ] TLA+ specifications
- [ ] Coq/Isabelle proofs
- [ ] HSM/KMS key management
- [ ] KES rotation
- [ ] Remote signer
- [ ] MPC multisig

#### Phase 7: Developer Platform & Ecosystem (Weeks 44-52)
- [ ] TypeScript SDK
- [ ] Python SDK
- [ ] Rust SDK enhancements
- [ ] WASM build toolchain
- [ ] R/W set analyzer
- [ ] CLI (aetherctl)
- [ ] Next.js explorer
- [ ] Browser wallet
- [ ] Hardware wallet integration
- [ ] Documentation
- [ ] Tutorials
- [ ] Testnet incentives

## Branch Strategy

Each major component gets its own branch:
- `phase1/component-name` - Phase 1 features
- `phase2/component-name` - Phase 2 features
- etc.

Branches merge to `main` after completion.

## Commit Message Format

```
feat(scope): description
fix(scope): description
docs(scope): description
test(scope): description
```

Scopes: crypto, consensus, ledger, runtime, networking, rpc, programs, ai-mesh, etc.

## Testing Strategy

- Unit tests for each component
- Integration tests for system interactions
- Property tests for invariants
- Benchmarks for performance-critical paths

## Current Branches

- `main` - Stable integrated code
- `phase1/ed25519-verification` - ✅ Merged
- `phase1/json-rpc-server` - ✅ Merged
- `phase1/ecvrf-leader-election` - 🔄 In Progress

## Next 5 Priorities

1. **ECVRF leader election** - Fair randomness for consensus
2. **BLS aggregation** - Efficient vote compression
3. **HotStuff consensus** - Full BFT with 2-phase voting
4. **WASM runtime** - Smart contract execution
5. **Parallel scheduler** - Utilize R/W sets for concurrency

## Acceptance Criteria

### Phase 1 Complete When:
- [ ] VRF-PoS leader selection working
- [ ] BLS vote aggregation functional
- [ ] HotStuff 2-chain finality implemented
- [ ] WASM VM executing contracts
- [ ] Parallel scheduler showing 2.5x+ speedup
- [ ] Basic P2P gossip working
- [ ] 4-node devnet running
- [ ] JSON-RPC serving requests

### Phase 2 Complete When:
- [ ] Staking bond/unbond working
- [ ] Governance proposals functional
- [ ] AMM swaps executing
- [ ] AIC mint/burn working
- [ ] Job escrow end-to-end demo
- [ ] Fee markets operational
- [ ] State rent implemented

## Timeline

**Target**: 30-52 weeks to full implementation per `trm.md`

- Weeks 0-2: Foundation ✅ Complete
- Weeks 2-8: Phase 1 🔄 In Progress (Week 0)
- Weeks 8-12: Phase 2
- Weeks 12-20: Phase 3 (AI Mesh)
- Weeks 20-28: Phase 4 (Networking & Performance)
- Weeks 28-36: Phase 5 (SRE & Ops)
- Weeks 36-44: Phase 6 (Security & Formal Methods)
- Weeks 44-52: Phase 7 (Developer Platform)

## Resources

- Main roadmap: `trm.md`
- System overview: `overview.md`
- Compliance audit: `COMPLIANCE_AUDIT.md`
- Robustness report: `ROBUSTNESS_REPORT.md`
- Current status: `STATUS.md`

