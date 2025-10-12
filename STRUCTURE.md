# Aether Repository Structure

Complete, production-ready blockchain repository for scaling to millions of users.

```
aether/
├── Cargo.toml                          # Workspace manifest with all crates
├── Makefile                            # Build, test, deploy commands
├── README.md                           # Project overview
├── LICENSE                             # Apache 2.0
├── .gitignore                          # Git ignore patterns
│
├── config/
│   └── genesis.toml                    # Genesis configuration (chain params, tokens)
│
├── docs/
│   └── architecture.md                 # System architecture documentation
│
├── crates/                             # Rust workspace crates
│   │
│   ├── node/                           # Main node orchestrator
│   ├── consensus/                      # VRF-PoS + HotStuff BFT
│   ├── ledger/                         # eUTxO++ state management
│   ├── runtime/                        # WASM VM + parallel scheduler
│   ├── mempool/                        # Transaction pool + fee market
│   ├── p2p/                            # Peer-to-peer networking
│   │
│   ├── networking/
│   │   ├── quic-transport/             # QUIC transport layer
│   │   └── gossipsub/                  # Pub/sub message propagation
│   │
│   ├── da/                             # Data availability
│   │   ├── turbine/                    # Turbine block broadcast
│   │   ├── erasure-coding/             # Reed-Solomon encoding
│   │   └── shreds/                     # Block fragment structures
│   │
│   ├── crypto/                         # Cryptographic primitives
│   │   ├── primitives/                 # Ed25519, SHA-256, BLAKE3
│   │   ├── vrf/                        # VRF leader election
│   │   ├── bls/                        # BLS12-381 vote aggregation
│   │   ├── kes/                        # Key-evolving signatures
│   │   └── kzg/                        # KZG polynomial commitments
│   │
│   ├── state/                          # State management
│   │   ├── merkle/                     # Sparse Merkle tree
│   │   ├── storage/                    # RocksDB persistence
│   │   └── snapshots/                  # State snapshots for fast sync
│   │
│   ├── programs/                       # Native programs (system contracts)
│   │   ├── staking/                    # SWR staking + rewards + slashing
│   │   ├── governance/                 # On-chain governance + proposals
│   │   ├── amm/                        # Constant product DEX
│   │   ├── job-escrow/                 # AI job marketplace + AIC escrow
│   │   ├── reputation/                 # Provider reputation oracle
│   │   └── aic-token/                  # AIC token (burn on use)
│   │
│   ├── verifiers/                      # Verification systems
│   │   ├── tee/                        # SEV-SNP/TDX attestation
│   │   ├── kzg-verifier/               # KZG opening verification
│   │   └── vcr-validator/              # VCR validation orchestration
│   │
│   ├── rpc/                            # RPC interfaces
│   │   ├── json-rpc/                   # Standard JSON-RPC 2.0 API
│   │   └── grpc-firehose/              # High-throughput block streaming
│   │
│   ├── types/                          # Canonical type definitions
│   ├── codecs/                         # Binary serialization (Borsh/Bincode)
│   ├── metrics/                        # Prometheus metrics
│   │
│   ├── sdk/
│   │   └── rust/                       # Rust SDK for applications
│   │
│   └── tools/
│       ├── cli/                        # aetherctl CLI
│       ├── keytool/                    # Key generation utility
│       ├── faucet/                     # Testnet faucet
│       ├── indexer/                    # Blockchain indexer (Postgres + GraphQL)
│       └── loadgen/                    # Load testing tool
│
├── ai-mesh/                            # AI service mesh
│   ├── runtime/                        # TEE-attested worker runtime
│   ├── router/                         # Job routing + provider selection
│   ├── receipts/                       # VCR generation
│   ├── attestation/
│   │   └── Dockerfile                  # SEV-SNP container image
│   └── models/
│       └── build-deterministic.sh      # Reproducible model builds
│
├── deploy/                             # Deployment configurations
│   ├── docker/
│   │   └── docker-compose.yml          # Local 4-node devnet
│   ├── k8s/
│   │   └── validator/
│   │       └── deployment.yaml         # Kubernetes validator deployment
│   └── terraform/
│       └── main.tf                     # Cloud infrastructure (AWS/GCP/Azure)
│
└── tests/
    └── integration_test.rs             # End-to-end integration tests
```

## Key Design Principles

### Modularity
- Each crate has single, well-defined responsibility
- Clean interfaces between components
- Minimal coupling, high cohesion

### Scalability
- Parallel execution via R/W set scheduling
- Erasure-coded block propagation (Turbine)
- Horizontal scaling via L2/app-chains
- External DA for data availability

### Security
- BFT consensus with 2/3 threshold
- TEE attestation for AI workers
- Crypto-economic proofs (KZG)
- Slashing for malicious behavior

### Performance
- 500ms slot time → 2s finality
- 5-20k TPS on L1 (parallel exec)
- GPU signature verification
- NVMe-backed state storage

### Verifiability
- Deterministic execution (WASM)
- Sparse Merkle state commitments
- TEE quotes + KZG trace proofs
- Challenge-response dispute game

## Component Connections

### Data Flow: User Transaction
```
User → Mempool → VRF Leader → Turbine Broadcast → Validators
→ Parallel Execution → State Update → BLS Votes → Finality
```

### Data Flow: AI Job
```
User Posts Job → Job Escrow (AIC locked) → Router Selects Provider
→ Provider Executes (TEE) → VCR Generated → On-Chain Submission
→ Challenge Window → Settlement (Burn AIC, Pay Provider)
```

### Cryptographic Stack
```
Transactions: Ed25519 signatures
Consensus Votes: BLS12-381 aggregation
Leader Election: ECVRF
Validator Keys: KES rotation
AI Traces: KZG commitments
State: Sparse Merkle Tree
```

## Quick Start

```bash
# Build
make build

# Generate validator keys
make keys

# Start 4-node devnet
make devnet

# Check status
make status

# Run tests
cargo test --all

# Load test
make loadtest
```

## Deployment

- **Devnet**: `docker compose up` (local 4-node cluster)
- **Testnet**: `kubectl apply -f deploy/k8s/testnet/`
- **Mainnet**: `terraform apply` (AWS/GCP infra) + Kubernetes validators

## Next Steps

1. Implement core protocol (consensus, ledger, runtime)
2. Build native programs (staking, AMM, job escrow)
3. Deploy AI mesh (TEE workers, VCR validation)
4. Launch testnet with ≥15 validators
5. Security audits + formal verification
6. Mainnet launch

---

This structure is designed to scale from laptop devnet to global mainnet with millions of users and billions of AI inference jobs.

