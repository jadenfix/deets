# Aether Implementation Roadmap

This roadmap describes the next delivery stages that would bring the current repository from a strong engineering prototype toward a more operationally complete project.

## Current Baseline

The repository already contains:

- a substantial Rust workspace covering node, consensus, runtime, storage, RPC, programs, verifiers, and tooling;
- AI-mesh services and verification-related components;
- SDK and web-client workspaces;
- Docker, Helm, Kubernetes, Terraform, and observability assets; and
- a GitHub Actions workflow that validates core Rust and container paths.

The roadmap below focuses on closing the gap between “implemented in the repository” and “fully repeatable, production-grade delivery.”

## 1. CI/CD Completion

Priority outcomes:

- add the TypeScript SDK and frontend workspaces to GitHub Actions;
- add explicit `cargo check --workspace --all-features` coverage if desired separately from lint/test;
- add documentation link and structure validation;
- publish versioned build artifacts and container images; and
- establish a release process that is reproducible from tags rather than manual operator steps.

## 2. Protocol Hardening

Priority outcomes:

- continue hardening consensus, slashing, runtime, and state-transition behavior;
- expand property testing, fuzzing, and long-running soak coverage;
- validate cross-crate integration between node, RPC, P2P, and storage paths; and
- document maturity boundaries between local/dev flows and higher-environment expectations.

## 3. AI Verification Hardening

Priority outcomes:

- tighten attestation verification and verifier integration;
- harden proof-validation boundaries and challenge/settlement paths;
- define clear criteria for when AI-mesh components are CI-required versus experimental; and
- ensure operational documentation matches the actual supported workflows for those services.

## 4. Deployment and Operations Maturity

Priority outcomes:

- validate Compose, Helm, Kubernetes, and Terraform assets against real target environments;
- define the supported deployment topologies explicitly;
- add repeatable operational procedures for recovery, rollback, secrets, and key handling; and
- close the gap between local devnet documentation and multi-environment deployment guidance.

## 5. Developer Experience

Priority outcomes:

- keep setup, contributing, and architecture docs synchronized with the repo;
- standardize the local command surface across scripts, Make targets, and package scripts;
- improve cross-language testing guidance for Rust, TypeScript, Python, and UI work; and
- make project status and roadmap documents evidence-based and easier for external contributors to trust.
