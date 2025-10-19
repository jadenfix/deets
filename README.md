# Aether: The AI Credits Superchain

> **Turn verifiable AI compute into programmable money.**  
> We are closing out Phase 2 of the roadmap by wiring the ledger, runtime, snapshots, and tooling together so Phase 3 program work can begin.

---

## Phase 2 Snapshot (October 2025)

- Ledger-backed runtime execution (`LedgerRuntimeState`) applies contract storage, balance deltas, and logs atomically.
- Chain store and receipt persistence (`crates/ledger/src/chain_store.rs`) power end-to-end integration coverage.
- Compressed snapshots import/export round-trip ledger state into fresh stores.
- Deterministic execution guards check ledger ordering and WASM gas usage.
- CLI smoke flow (keygen → status → transfer → job post → delegate) keeps developer ergonomics healthy.
- `scripts/phase2_acceptance_test.sh` bundles the entire stack into a single gate that now passes locally.

Phase 1 remains green via `scripts/quick-check.sh`. Phases 3–7 intentionally stay untouched until Phase 2 merges.

---

## Key Components (Delivered)

- **Consensus scaffolding** – Hybrid VRF + HotStuff framework with validator accounting (`crates/consensus`).
- **Ledger core** – UTxO + account hybrid ledger, batch signature verification, contract storage helpers (`crates/ledger`).
- **Runtime** – Scheduler, Wasm VM, host functions, and the new ledger-backed runtime state (`crates/runtime`).
- **State services** – RocksDB-backed storage, Sparse Merkle tree, compressed snapshot pipeline (`crates/state/*`).
- **Tooling** – Minimal CLI (`crates/tools/cli`) plus determinism and cross-crate integration suites under `tests/`.

See `progress.md` for a living status board with links into each deliverable.

---

## Validating the Build

Run the Phase 2 acceptance harness:

```bash
./scripts/phase2_acceptance_test.sh
```

The script enforces formatting, builds the workspace, runs targeted crate suites, exercises the cross-crate integration tests, checks determinism, and finishes with the CLI smoke test. All steps succeed on `feature/phase2-testing`.

For a faster Phase 1 sanity sweep:

```bash
./scripts/quick-check.sh
```

---

## Developer Workflow

```bash
# Clone the repo
git clone https://github.com/jadenfix/deets.git
cd deets

# Compile everything
cargo build --workspace

# Run the cross-crate regression
cargo test --test phase2_integration

# Run determinism guardrails
cargo test --test determinism_test
```

Both integration suites rely on temporary in-process ledgers and require no external services.

---

## Roadmap (Next Up)

1. Merge the Phase 2 branch after review and CI wiring.
2. Scope Phase 3 (staking, governance, AMM, job escrow logic backed by the new runtime plumbing).
3. Expand CI to include `scripts/phase2_acceptance_test.sh` and sketch program-level acceptance tests.

For the full seven-phase roadmap and architectural context, check:
- `overview.md`
- `trm.md`
- `IMPLEMENTATION_ROADMAP.md`

---

## License

Apache 2.0 – build, fork, and deploy with confidence.
