# Contributing to Aether

## Development Setup

1. Clone and enter repo:
   ```bash
   git clone https://github.com/jadenfix/deets.git
   cd deets
   ```
2. Run baseline checks:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo check --workspace
   ```

## Test Commands

- Phase acceptance suites:
  - `./scripts/run_phase1_acceptance.sh`
  - `./scripts/run_phase2_acceptance.sh`
  - `./scripts/run_phase3_acceptance.sh`
  - `./scripts/run_phase4_acceptance.sh`
  - `./scripts/run_phase5_acceptance.sh`
  - `./scripts/run_phase6_acceptance.sh`
  - `./scripts/run_phase7_acceptance.sh`
- Targeted Rust crate tests:
  ```bash
  cargo test -p <crate-name>
  ```

## Branching and Commits

- Prefer topic branches: `phaseX/<feature-name>` or `feat/<scope>-<name>`.
- Commit format:
  - `feat(scope): description`
  - `fix(scope): description`
  - `docs(scope): description`
  - `test(scope): description`

## Pull Request Expectations

1. Describe behavior changes and affected crates/apps.
2. Include validation steps and command output summary.
3. Add/adjust tests for new behavior.
4. Update docs when interfaces, scripts, or status claims change.
5. Keep PRs scoped; split unrelated refactors into separate PRs.

## Code Quality Notes

- Avoid introducing placeholder production paths (`TODO`/`stub`) without explicit feature flags.
- Prefer deterministic behavior in consensus/runtime/crypto code paths.
- Keep SDK and CLI behavior aligned with live RPC contracts.
