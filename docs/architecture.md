# Aether Architecture

## System Overview

Aether is a high-performance L1 blockchain designed for verifiable AI compute with:

- **Consensus**: VRF-PoS + HotStuff BFT (500ms finality)
- **Execution**: eUTxO++ with parallel scheduling (Sealevel-style)
- **Networking**: QUIC + Turbine erasure-coded broadcast
- **AI Mesh**: TEE-attested workers with verifiable compute receipts

## Component Architecture

### Core Protocol Stack

```
┌─────────────────────────────────────────────────────────────┐
│                       USER APPLICATIONS                      │
├─────────────────────────────────────────────────────────────┤
│  Wallets  │  Explorers  │  DApps  │  AI Clients  │  ...    │
└─────────────────────────────────────────────────────────────┘
                            ↕ JSON-RPC / gRPC
┌─────────────────────────────────────────────────────────────┐
│                         RPC LAYER                            │
│  JSON-RPC (port 8545)  │  gRPC Firehose (port 8546)         │
└─────────────────────────────────────────────────────────────┘
                            ↕
┌─────────────────────────────────────────────────────────────┐
│                        NODE CORE                             │
├─────────────────────────────────────────────────────────────┤
│  Mempool  →  Consensus (VRF+HotStuff)  →  Block Production  │
│     ↓                                                         │
│  Runtime (WASM + Parallel Scheduler)  →  State Updates      │
│     ↓                                                         │
│  Ledger (eUTxO++ + Merkle Tree)  →  RocksDB Storage         │
└─────────────────────────────────────────────────────────────┘
                            ↕ P2P (QUIC + Gossipsub)
┌─────────────────────────────────────────────────────────────┐
│                      NETWORKING LAYER                        │
│  Gossipsub (tx, header, vote)  │  Turbine (shreds)          │
└─────────────────────────────────────────────────────────────┘
```

### AI Service Mesh

```
┌─────────────────────────────────────────────────────────────┐
│                        USER / DAPP                           │
└─────────────────────────────────────────────────────────────┘
                            ↓ Post Job
┌─────────────────────────────────────────────────────────────┐
│                    JOB ESCROW (On-Chain)                     │
│  AIC Locked  →  Provider Selected  →  Bond Staked           │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│                       JOB ROUTER                             │
│  Reputation Query  →  Provider Matching  →  Assignment       │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│                    AI WORKER (TEE)                           │
│  Execute in SNP/TDX  →  Generate Attestation  →  VCR        │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│                   VCR VALIDATION (On-Chain)                  │
│  TEE Quote  →  KZG Commits  →  Challenge Window  →  Settle  │
└─────────────────────────────────────────────────────────────┘
```

## Data Flow

### Transaction Flow

1. User submits tx → Mempool (gossipsub 'tx')
2. VRF leader election → Block proposal
3. Turbine broadcast (RS erasure shreds)
4. Parallel execution (R/W set scheduler)
5. BLS vote aggregation → Finality
6. State commit → Merkle root → Receipts

### AI Job Flow

1. User posts job (AIC escrowed)
2. Router selects provider (reputation-based)
3. Provider executes in TEE → VCR generation
4. VCR submitted on-chain → Challenge window
5. Watchtower verification (optional challenge)
6. Settlement: Burn AIC, pay provider, return bond

## Scale Architecture

- **L1**: 5-20k TPS (parallel exec, 2-4MB blocks)
- **L2/App-chains**: IBC-like async messaging
- **External DA**: Celestia/Avail for data availability
- **Edge RPC**: Anycast + ZK light clients
- **Payment Channels**: AIC streaming for micro-payments

## Security Model

- **Consensus**: 2/3 BFT threshold, VRF unpredictability
- **Execution**: Deterministic WASM, no side effects
- **AI Verification**: TEE attestation + KZG crypto-economic proofs
- **Slashing**: Double-sign (5%), downtime (gradual leak)

