# Aether Technical Roadmap

This document expands on the implementation roadmap with a more technical view of the major workstreams and their intended acceptance signals.

## Objectives

- Keep the protocol and runtime path deterministic, reviewable, and well-tested.
- Align developer-facing docs and tooling with the actual repository state.
- Expand delivery automation beyond the current Rust and Docker validation lanes.
- Clarify which deployment and AI-mesh surfaces are production candidates versus experimental assets.

## Workstream 1: Core Protocol

Focus areas:

- consensus, slashing, and vote-handling correctness;
- runtime and scheduler behavior under conflicting and non-conflicting workloads;
- storage, state commitment, and recovery behavior; and
- integration between node, RPC, P2P, and persistence layers.

Acceptance signals:

- regression coverage for critical code paths;
- stable local and Compose-based network flows;
- explicit docs for configuration, failure modes, and supported environments.

## Workstream 2: Interfaces and Tooling

Focus areas:

- JSON-RPC and downstream indexing expectations;
- CLI ergonomics and configuration clarity;
- SDK alignment with the current RPC surface; and
- predictable local workflows for developers working across Rust and web packages.

Acceptance signals:

- CLI, SDK, and setup docs reference real commands and current APIs;
- interface changes include tests and documentation updates;
- contributor guidance stays synchronized with CI.

## Workstream 3: AI Mesh and Verification

Focus areas:

- attestation and verifier boundaries;
- integration between on-chain job flows and off-chain services;
- operational clarity around what is required versus optional; and
- evidence-based documentation of supported verification paths.

Acceptance signals:

- verifier and AI-mesh behavior is documented against current code, not target-state aspirations;
- CI expectations for those components are explicit;
- audit scope and threat-model documents reflect the codebase layout.

## Workstream 4: Delivery and Operations

Focus areas:

- broader CI coverage, including non-Rust workspaces;
- release artifact strategy and image publication;
- validation of `deploy/docker`, `deploy/helm`, `deploy/k8s`, and `deploy/terraform`; and
- operational runbooks tied to the environments that the repository actually supports.

Acceptance signals:

- deployment assets have a documented validation story;
- release handling is repeatable and traceable;
- runbooks and architecture docs agree on environment boundaries.

## Workstream 5: Documentation Quality

Focus areas:

- eliminate drift between markdown documents and current code;
- replace speculative status claims with concrete, verifiable statements;
- distinguish CI coverage from manual/operator workflows; and
- give contributors a consistent map of the repository.

Acceptance signals:

- top-level docs match current commands, scripts, and workflow jobs;
- architecture and operations docs describe the same system model;
- security and audit documents reference the actual code and deployment surfaces under review.
