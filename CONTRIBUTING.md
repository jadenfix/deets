# Contributing to Aether

This document describes the current contribution expectations for the repository as it exists today. The goal is to keep changes reviewable, verifiable, and aligned with the code paths that are actually exercised in CI.

## Development Environment

Core requirements:

- Rust `1.86.0` with `rustfmt` and `clippy`, matching CI.
- Git.
- Docker and Docker Compose for Compose-based validation and local network flows.

Optional requirements:

- Node.js 20+ and `npm` if you touch `sdks/typescript`, `packages/ui`, `apps/explorer`, or `apps/wallet`.

## Branching

- Prefer a short-lived topic branch or a separate git worktree for each unit of work.
- Keep one pull request focused on one change set. Do not mix protocol changes, frontend work, and documentation rewrites unless they are tightly coupled.
- Avoid force-pushing rebased history during active review unless the PR explicitly needs it.

## Validation Matrix

Match the validation to the surface you change.

For most Rust changes:

```bash
./scripts/lint.sh
./scripts/test.sh
```

For Compose-based integration work:

```bash
./scripts/docker-test.sh
```

For phase-specific changes:

```bash
./scripts/run_phase1_acceptance.sh
./scripts/run_phase2_acceptance.sh
./scripts/run_phase3_acceptance.sh
./scripts/run_phase4_acceptance.sh
./scripts/run_phase5_acceptance.sh
./scripts/run_phase6_acceptance.sh
./scripts/run_phase7_acceptance.sh
```

For TypeScript SDK or frontend changes:

```bash
npm run test:ts
```

If you touch deployment or operations assets, verify the relevant files directly:

- `.github/workflows/ci.yml`
- `docker-compose.test.yml`
- `deploy/docker/docker-compose.yml`
- `deploy/helm/`
- `deploy/k8s/`
- `deploy/terraform/`

## Change Expectations

- Add or update tests when behavior changes.
- Update docs when commands, architecture, interfaces, or operational guidance change.
- Keep README and getting-started material factual. Do not add aspirational claims as current capability.
- Prefer targeted edits over broad refactors that are unrelated to the task.
- Preserve deterministic behavior and explicit error handling in consensus, runtime, cryptography, and state-transition code.

## Pull Request Checklist

1. Describe the user-visible or operator-visible impact.
2. List the commands you ran to validate the change.
3. Call out any follow-up work or intentionally deferred items.
4. Update documentation if the change affects setup, operation, APIs, or project status.
5. Note any risk areas, especially for consensus, runtime, RPC, storage, or key-management paths.

## Commit Style

Use a concise conventional-style subject when possible:

- `feat(scope): description`
- `fix(scope): description`
- `docs(scope): description`
- `test(scope): description`
- `refactor(scope): description`

Useful scopes in this repository include `consensus`, `runtime`, `ledger`, `rpc`, `programs`, `ai-mesh`, `ops`, `sdk`, `ui`, and `docs`.

## Documentation Standard

Project documentation should:

- describe the repository as it is, not as it might become;
- cite real commands, scripts, files, and workflows from the repo;
- distinguish between CI-validated paths and operator-driven deployment assets; and
- stay readable for external contributors who are new to the codebase.
