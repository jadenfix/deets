# Aether Overview

Aether is a multi-surface repository that combines a Rust blockchain implementation, AI verification components, developer tooling, and deployment assets in one workspace. This document provides a short map of the project and points to the detailed reference material.

## What the Repository Contains

### Core Protocol

The `crates/` workspace contains the node, consensus, ledger, runtime, mempool, RPC, networking, state, and cryptography components. These crates make up the primary execution path for the chain.

### On-Chain Programs and Verifiers

The repository also includes native program crates for staking, governance, AMM, job escrow, reputation, token logic, account abstraction, and related verification modules for AI and cryptographic proof flows.

### AI Mesh

The `ai-mesh/` directory holds the off-chain service side of the project: runtime, router, coordinator, worker, models, and attestation assets.

### Tooling and Interfaces

The repository ships developer-facing interfaces across multiple layers:

- `aetherctl`, keytool, faucet, indexer, scorecard, and load generator under `crates/tools/`
- Rust SDK support under `crates/sdk/`
- TypeScript and Python SDKs under `sdks/`
- Explorer and wallet applications under `apps/`

### Deployment Assets

Deployment and observability material is kept under `deploy/` and includes:

- Docker build and Compose assets
- Helm charts
- Kubernetes manifests
- Terraform scaffolding
- Prometheus and Grafana configuration

## System Model

The current repository models Aether as a layered system:

1. Clients and tools submit transactions or query data over JSON-RPC and gRPC-style interfaces.
2. The node accepts transactions, manages mempool state, and drives consensus and execution.
3. Runtime and ledger crates apply deterministic state transitions and persist chain data.
4. P2P, QUIC, and data-availability crates distribute transactions, votes, and block data.
5. AI mesh components coordinate attested compute workflows that connect to on-chain job and verification logic.

For the deeper component view, use `docs/architecture.md`.

## Current Delivery Model

The repository has strong CI coverage for the Rust and Docker validation paths:

- linting and security audit;
- workspace tests and doc tests;
- multi-architecture release builds;
- container build validation;
- Compose-based integration testing; and
- phase acceptance scripts.

What it does not do yet is automated release publishing or automated production deployment from GitHub Actions. Deployment remains an operator workflow built around the assets under `deploy/`.

## Recommended Reading Order

1. `README.md` for the project summary and quick start.
2. `GETTING_STARTED.md` for setup and local workflows.
3. `docs/architecture.md` for the system design.
4. `progress.md` for the current repository status.
5. `IMPLEMENTATION_ROADMAP.md` and `trm.md` for the planned delivery path.
6. `docs/ops/RUNBOOKS.md` and `docs/security/` for operational and audit context.
