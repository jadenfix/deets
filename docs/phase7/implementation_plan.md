# Phase 7 Completion Plan

This document captures the implementation breakdown for the remaining Phase 7 deliverables so that the DX team can work in parallel and land the final acceptance artifacts.

## 1. aetherctl Enhancements

### Goals
- Turn the CLI into a first-class interface for transfers, staking, and AI job lifecycle.
- Reuse the Rust SDK builders to guarantee identical transaction/job encoding.
- Provide deterministic output that can be asserted in tests and scripted in CI.

### Tasks
1. Introduce a `Config` loader (`~/.aether/config.toml`) with endpoint + key store paths.
2. Replace the static `main.rs` output with a Clap-based command graph:
   - `aetherctl status`
   - `aetherctl keys generate --out <file>` (Ed25519)
   - `aetherctl transfer --to <addr> --amount <amt> --nonce <n> [--memo <txt>]`
   - `aetherctl job post --model <hash> --input <hash> --max-fee <amt> --expires <ts>`
   - `aetherctl job tutorial` (prints the hello flow with curl payloads)
3. Implement signing helpers that read a keypair file and feed it into the SDK transfer builder.
4. Add `tests/cli_tests.rs` with `assert_cmd` harness to ensure:
   - Config fallback works.
   - Transfer/job commands emit submission JSON matching the SDK reference values.

## 2. Explorer & Wallet Scaffolding

### Goals
- Deliver a browsable chain overview and a minimal wallet that share component primitives.
- Provide a TypeScript workspace with lint/test tasks runnable from CI.

### Tasks
1. Create `apps/explorer` (Vite + React + TypeScript):
   - Pages: `ChainOverview`, `Validators`, `Jobs`.
   - Hooks reading from an RPC mock service (fetch wrapper that falls back to fixtures).
   - Vitest tests covering rendering & data transforms.
2. Create `apps/wallet` sharing UI kit from `packages/ui`:
   - Views: `Account`, `Transfer`, `JobSubmission`.
   - Client uses the TypeScript SDK to build payloads; display JSON preview.
   - Vitest tests for form validation and payload generation.
3. Establish a shared ESLint/TSConfig in `apps/tsconfig.base.json` and ensure both apps reference it.
4. Add `pnpm` workspace file `package.json` at repo root (or extend existing Node tooling) so the new packages can be linted/tested via Phase 7 acceptance.

## 3. Grants & Testnet Incentives

### Goals
- Provide automation for faucets and validator scorecards.
- Publish documentation guiding contributors through the incentive programs.

### Tasks
1. Flesh out `crates/tools/faucet` as an async Axum server with:
   - `/request` endpoint performing rate limiting and verifying GitHub handles.
   - Configurable per-token limits; writes audit entries to disk.
   - Unit tests using `axum::Router` + `tower::ServiceExt`.
2. Add `crates/tools/scorecard` (new crate) that pulls Prometheus JSON, calculates uptime/latency, and outputs markdown/CSV.
   - Include CLI entry point plus integration test with fixture metrics.
3. Author `docs/grants/overview.md` describing faucet usage, validator scoring, bug bounty submission, and scoreboard integration.
4. Provide automation scripts under `scripts/` to run faucet locally and generate weekly scorecards.

## 4. CI & Acceptance

### Goals
- Extend Phase 7 acceptance to cover Rust CLI tests, TypeScript app tests, and tooling binaries.
- Ensure GitHub Actions remain deterministic and resource-light.

### Tasks
1. Update `scripts/run_phase7_acceptance.sh` to:
   - Run `cargo test -p aether-cli` (after adding tests).
   - Build/test the TypeScript workspace (`pnpm install && pnpm test`).
   - Execute faucet + scorecard unit tests (`cargo test -p aether-faucet`, `cargo test -p aether-scorecard`).
2. Split caches to respect new Node workspace directories.
3. Expand documentation (`README.md`, `progress.md`) to mark Phase 7 as feature-complete once these tasks are merged.

---

The steps above map directly to the outstanding items in `trm.md` §10. Sequencing recommendation:
1. Land CLI + faucet updates (Rust focus).
2. Introduce TypeScript workspace (explorer/wallet).
3. Wire up CI/test coverage.
