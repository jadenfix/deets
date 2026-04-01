# Aether

Aether is a Rust-first monorepo for an L1 blockchain and related AI-compute infrastructure. The repository includes the validator/node, consensus and runtime crates, system programs, verifiers, SDKs, web clients, and deployment assets used for local development and infrastructure experiments.

## Repository Scope

- Core protocol: node, consensus, ledger, mempool, runtime, storage, networking, data-availability, and RPC crates.
- On-chain programs: staking, governance, AMM, job escrow, reputation, account abstraction, and token modules.
- AI mesh: attestation, router, coordinator, worker, runtime, and verifier crates.
- Developer interfaces: Rust, TypeScript, and Python SDKs; `aetherctl`; faucet, keytool, indexer, and load generator.
- Applications and shared UI: explorer, wallet, and shared frontend packages.
- Deployment assets: Dockerfiles, Compose stacks, Helm charts, Kubernetes manifests, Terraform, Prometheus, and Grafana assets.

## Architecture Summary

At a high level, Aether is organized as a layered system:

1. Interface layer: JSON-RPC, gRPC/firehose, SDKs, CLI, explorer, and wallet.
2. Node core: transaction ingress, mempool, consensus, runtime, ledger, and state storage.
3. Network and data plane: libp2p/gossipsub, QUIC transport, and Turbine-style data-distribution crates.
4. AI verification path: attestation, verifiers, and AI mesh services that integrate with on-chain job flows.
5. Operations layer: metrics, indexer, Compose environments, and deployment scaffolding under `deploy/`.

The current node binary runs a hybrid consensus path built around VRF-based leader selection, HotStuff-style voting/finality, and BLS-backed vote handling. The runtime and ledger crates provide deterministic state transition logic, while the repository also contains supporting infrastructure for AI job verification and downstream indexing.

## CI/CD Posture

The repository currently has one primary GitHub Actions workflow at `.github/workflows/ci.yml`. It runs on pushes to `main` and `release/**`, and on pull requests targeting `main`.

Current CI jobs:

- `lint`: `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo audit`.
- `test`: workspace unit tests and doc tests.
- `build`: release builds for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`.
- `docker`: container build validation.
- `integration`: single-node container smoke via `docker-compose.test.yml`, including RPC health checks and a workspace test run from the test-runner container.
- `phase-acceptance`: `scripts/run_phase{1..7}_acceptance.sh` when those scripts are present.

Deployment assets exist under `deploy/`, but GitHub Actions does not currently publish releases or perform automated environment rollouts. Operational deployment remains operator-driven.

## Quick Start

### Prerequisites

- Rust `1.86.0` with `rustfmt` and `clippy`, matching the pinned CI toolchain.
- Docker and Docker Compose if you want to use the Compose-based dev/test environments.
- Node.js 20+ and `npm` only if you plan to work on the TypeScript SDK or web applications.

### Build and Run a Node

```bash
git clone https://github.com/jadenfix/deets.git
cd deets

cargo build --workspace
cargo run -p aether-node
```

By default the node starts in the devnet preset, stores data under `./data/node1`, exposes JSON-RPC on `127.0.0.1:8545`, and keeps running until interrupted.

You can query the node with JSON-RPC:

```bash
curl -s http://127.0.0.1:8545 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"aeth_getSlotNumber","params":[],"id":1}'
```

### Common Local Workflows

```bash
# CI-aligned lint/check flow
./scripts/lint.sh

# CI-aligned test flow
./scripts/test.sh

# Local multi-process devnet
./scripts/devnet.sh

# Docker-based container smoke
./scripts/docker-test.sh

# Optional TypeScript/web test lane
npm run test:ts
```

### CLI

The repository includes `aetherctl` in `crates/tools/cli`:

```bash
cargo run -p aether-cli --bin aetherctl -- --help
```

## Repository Layout

```text
crates/           Rust protocol, runtime, program, RPC, tooling, and verifier crates
ai-mesh/          AI execution and coordination services
apps/             Explorer and wallet applications
sdks/             Python and TypeScript SDKs
packages/         Shared frontend packages
scripts/          Lint, test, devnet, Docker, and acceptance helpers
deploy/           Docker, Helm, Kubernetes, Terraform, Prometheus, and Grafana assets
docs/             Architecture, operations, security, and project reference material
```

## Documentation Map

- `GETTING_STARTED.md`: contributor bootstrap and local workflows.
- `overview.md`: high-level repository and system overview.
- `docs/architecture.md`: current system design and deployment surfaces.
- `progress.md`: status snapshot of what is present in the repository and what CI validates today.
- `IMPLEMENTATION_ROADMAP.md`: forward-looking delivery plan aligned to the current codebase.
- `trm.md`: technical roadmap and workstreams.
- `docs/ops/RUNBOOKS.md`: operational procedures for local and Compose-based environments.
- `docs/security/AUDIT_SCOPE.md`: audit scoping for the current repository.

## Contributing

Contributions are welcome across protocol, tooling, frontend, AI mesh, operations, and documentation work. Use [CONTRIBUTING.md](./CONTRIBUTING.md) for the expected validation flow and PR standards.

## License

This repository is licensed under the terms in [LICENSE](./LICENSE).
