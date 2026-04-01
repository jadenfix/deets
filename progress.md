# Aether Status Snapshot

**Date**: March 31, 2026

## Summary

Aether is in active development. The repository already contains the principal protocol crates, on-chain programs, AI-mesh components, SDKs, web clients, and deployment assets, and the current GitHub Actions workflow continuously validates the Rust and container paths. The project documentation should therefore describe a substantial in-repo implementation, but it should not describe the repository as having automated release or production deployment workflows when those are not present.

## Present in the Repository

### Protocol and Runtime

- Node, consensus, ledger, mempool, RPC, runtime, networking, data-availability, storage, and state crates are present in the Rust workspace.
- The node binary currently assembles a VRF plus HotStuff plus BLS consensus path and exposes JSON-RPC locally on port `8545` by default.
- Supporting cryptography crates for Ed25519, BLS, VRF, KES, and KZG are present.

### Programs and Verifiers

- The workspace includes staking, governance, AMM, job escrow, reputation, token, account-abstraction, and rollup-oriented crates.
- Verifier crates and AI-related proof/attestation paths are present alongside the protocol code.

### AI Mesh and Tooling

- `ai-mesh/` contains router, coordinator, runtime, worker, and attestation/model assets.
- The repository includes developer tools such as `aetherctl`, faucet, keytool, scorecard, indexer, and load generator.
- TypeScript and Python SDKs, plus explorer and wallet applications, are included in the monorepo.

### Operations Assets

- Dockerfiles and Compose files are present for local and test workflows.
- Helm, Kubernetes, Terraform, Prometheus, and Grafana assets exist under `deploy/`.

## What Automation Validates Today

The current GitHub Actions workflow validates:

- formatting, linting, and `cargo audit`;
- workspace unit tests and doc tests;
- Linux release builds for `x86_64` and `aarch64`;
- Docker image buildability;
- a Compose-based integration environment from `docker-compose.test.yml`; and
- phase acceptance scripts when the relevant script files exist.

This is meaningful CI coverage, but it is not a full release pipeline.

## Current Gaps in Delivery Automation

- GitHub Actions does not currently run the TypeScript or frontend test lane.
- GitHub Actions does not publish binaries, images, or versioned release artifacts.
- Deployment remains manual/operator-driven despite the presence of Helm, Kubernetes, and Terraform assets.
- Deployment manifests and higher-environment workflows still require environment-specific validation outside CI.

## Documentation Standard Going Forward

Project status documents should:

- distinguish between code that exists in the repository and workflows that are actively automated;
- avoid unsupported metrics, maturity percentages, or launch-readiness claims unless they are backed by current evidence; and
- stay synchronized with `.github/workflows/ci.yml`, `scripts/`, `docker-compose.test.yml`, and the deployment assets under `deploy/`.
