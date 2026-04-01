# Aether Architecture

This document describes the current repository-level system design and the deployment surfaces that exist in the codebase today.

## Design Goals

- deterministic execution and explicit state transitions;
- clear separation between protocol, interface, AI verification, and operations concerns;
- local-first developer workflows that can run without external infrastructure; and
- deployment assets that can evolve from local environments toward managed infrastructure.

## System Context

```text
Clients / CLI / SDKs / Apps
            |
            v
    JSON-RPC / gRPC surfaces
            |
            v
  Node ingress -> mempool -> consensus -> runtime -> ledger -> state/storage
            |                    |                      |
            |                    |                      +-> Merkle/state backends
            |                    +-> validator voting / finality
            +-> P2P, QUIC, DA, gossip

AI mesh services and verifiers sit alongside the protocol path and integrate with
job-related program logic, proof validation, and downstream tooling.
```

## Major Repository Domains

### Interfaces

- `crates/rpc/json-rpc`: JSON-RPC server and WebSocket subscription surface.
- `crates/rpc/grpc-firehose`: downstream event and indexing-oriented interface surface.
- `crates/tools/cli`: `aetherctl`.
- `sdks/`, `apps/`, and `packages/`: SDK and web-client layers.

The JSON-RPC server currently binds to `127.0.0.1` by default and exposes `/health`, POST JSON-RPC, and `/ws`.

### Node Core

- `crates/node`: binary entrypoint, node orchestration, and environment-driven configuration.
- `crates/mempool`: transaction staging and ordering.
- `crates/consensus`: leader election, voting, slashing, and consensus logic.
- `crates/runtime`: execution engine and scheduling logic.
- `crates/ledger`: state transition and block application logic.
- `crates/state/*`: storage and state-commitment backends.

The current binary assembles a hybrid path that combines VRF-oriented leader selection, HotStuff-style voting/finality, BLS-backed signatures, and ledger/runtime processing.

### Networking and Data Distribution

- `crates/p2p`
- `crates/networking/quic-transport`
- `crates/networking/gossipsub`
- `crates/da/*`

These crates provide the networking and data plane for peer connectivity, gossip, QUIC transport, shreds, and erasure/data-availability support.

### Programs and Verifiers

- `crates/programs/*`: staking, governance, AMM, job escrow, reputation, token, and related logic.
- `crates/verifiers/*`: verification crates for attestation and proof-related paths.

### AI Mesh

- `ai-mesh/runtime`
- `ai-mesh/router`
- `ai-mesh/coordinator`
- `ai-mesh/worker`
- `ai-mesh/attestation`

These components model the off-chain service layer for AI execution, routing, coordination, and attestation-related work.

### Tooling and Observability

- `crates/metrics`
- `crates/tools/indexer`
- `crates/tools/faucet`
- `crates/tools/loadgen`
- `deploy/prometheus`
- `deploy/grafana`

## Execution Flow

At a high level:

1. A client or tool submits a transaction over JSON-RPC.
2. The node validates and stages the transaction in the mempool.
3. Consensus chooses or confirms the block-production path.
4. Runtime and ledger apply deterministic state transitions.
5. State roots, receipts, and downstream interfaces expose the resulting chain data.
6. P2P and data-distribution components propagate blocks, votes, and related payloads.

AI-related flows extend this model by pairing on-chain job/program logic with off-chain AI mesh services and verifier components.

## Deployment Surfaces

The repository currently includes several environment shapes:

### Single-Node Local Process

- `cargo run -p aether-node`
- best for local development and direct RPC inspection

### Multi-Node Local Devnet

- `scripts/devnet.sh`
- launches four local validator processes with separate RPC and P2P ports

### Compose-Based Test Network

- `docker-compose.test.yml`
- used by CI to stand up a single containerized node, perform RPC smoke checks, and run workspace tests from a test-runner container

### Compose-Based Development Stack

- `deploy/docker/docker-compose.yml`
- includes a node, indexer, Prometheus, Grafana, and MinIO assets

### Infrastructure Scaffolding

- `deploy/helm/`
- `deploy/k8s/`
- `deploy/terraform/`

These assets represent deployment intent and infrastructure direction, but they are not the same as a fully automated release or rollout pipeline.

## CI Alignment

The architecture and project docs should stay aligned with the actual workflow in `.github/workflows/ci.yml`.

Today that workflow validates:

- linting and security audit;
- workspace tests and doc tests;
- multi-architecture Linux release builds;
- Docker buildability;
- a Compose-based container smoke flow; and
- phase acceptance scripts.

It does not currently perform artifact publication or automated deployment.
